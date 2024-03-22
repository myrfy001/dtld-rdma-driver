use buddy_system_allocator::LockedHeap;

use eui48::MacAddress;
use open_rdma_driver::{
    types::{MemAccessTypeFlag, Pmtu, QpType, RdmaDeviceNetwork},
    Device, Mr, Pd, Qp, Sge,
};
use std::time::{self};
use std::{ffi::c_void, net::Ipv4Addr};

const ORDER: usize = 32;
const SHM_PATH: &str = "/bluesim1\0";

#[macro_use]
extern crate ctor;

/// Use `LockedHeap` as global allocator
#[global_allocator]
static HEAP_ALLOCATOR: LockedHeap<ORDER> = LockedHeap::<ORDER>::new();
const HEAP_BLOCK_SIZE: usize = 1024 * 1024 * 64;

static mut HEAP_START_ADDR: usize = 0;

#[ctor]
fn init_global_allocator() {
    unsafe {
        let shm_fd = libc::shm_open(
            SHM_PATH.as_ptr() as *const libc::c_char,
            libc::O_RDWR,
            0o600,
        );

        let heap = libc::mmap(
            std::ptr::null_mut::<c_void>(),
            1024 * 1024 * 1024,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            shm_fd,
            0,
        );

        HEAP_START_ADDR = heap as usize;

        HEAP_ALLOCATOR.lock().init(heap as usize, HEAP_BLOCK_SIZE);
    }
}

fn create_and_init_card(
    card_id: usize,
    mock_server_addr: &str,
    network: RdmaDeviceNetwork,
) -> (Device, Pd, Mr, Qp, Box<[u8]>) {
    let head_start_addr = unsafe { HEAP_START_ADDR };
    let dev =
        Device::new_emulated(mock_server_addr.parse().unwrap(), head_start_addr, &network).unwrap();
    eprintln!("[{}] Device created", card_id);

    let pd = dev.alloc_pd().unwrap();
    eprintln!("[{}] PD allocated", card_id);

    let mut mr_buffer = Box::new([0u8; 1024 * 256]);

    unsafe {
        println!(
            "[{}] MR's PA_START={:X}",
            card_id,
            mr_buffer.as_mut_ptr() as usize - HEAP_START_ADDR
        );
    }

    let access_flag = MemAccessTypeFlag::IbvAccessRemoteRead
        | MemAccessTypeFlag::IbvAccessRemoteWrite
        | MemAccessTypeFlag::IbvAccessLocalWrite;
    let mr = dev
        .reg_mr(
            pd.clone(),
            mr_buffer.as_mut_ptr() as u64,
            mr_buffer.len() as u32,
            1024 * 1024 * 2,
            access_flag,
        )
        .unwrap();
    eprintln!("[{}] MR registered", card_id);

    let qp = dev
        .create_qp(
            pd.clone(),
            QpType::Rc,
            Pmtu::Mtu4096,
            access_flag,
            network.ipaddr,
            network.macaddr,
        )
        .unwrap();
    eprintln!("[{}] QP created", card_id);

    (dev, pd, mr, qp, mr_buffer)
}
#[allow(unused)]
fn main() {
    const SEND_CNT: usize = 8192 * 4;
    let a_network = RdmaDeviceNetwork {
        gateway: Ipv4Addr::new(192, 168, 0, 0x1),
        netmask: Ipv4Addr::new(255, 255, 255, 0),
        ipaddr: Ipv4Addr::new(192, 168, 0, 2),
        macaddr: MacAddress::new([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFE]),
    };
    let b_network = RdmaDeviceNetwork {
        gateway: Ipv4Addr::new(192, 168, 0, 0x1),
        netmask: Ipv4Addr::new(255, 255, 255, 0),
        ipaddr: Ipv4Addr::new(192, 168, 0, 3),
        macaddr: MacAddress::new([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]),
    };
    let (dev_a, _pd_a, mr_a, qp_a, mut mr_buffer_a) =
        create_and_init_card(0, "0.0.0.0:9873", a_network);
    let (dev_b, _pd_b, mr_b, qp_b, mut mr_buffer_b) =
        create_and_init_card(1, "0.0.0.0:9875", b_network);
    
    // qp communication
    dev_a.add_remote_qp(&qp_b);
    dev_b.add_remote_qp(&qp_a);
    
    // fill mr_buffer with some data
    let current_time = time::SystemTime::now()
        .duration_since(time::UNIX_EPOCH)
        .expect("Time went backwards");
    let timestamp = current_time.as_secs();
    mr_buffer_a[..8].copy_from_slice(&timestamp.to_le_bytes());
    for (idx, item) in mr_buffer_a[8..].iter_mut().enumerate() {
        *item = idx as u8;
    }
    for item in mr_buffer_b[0..].iter_mut() {
        *item = 0
    }

    let sge0 = Sge {
        addr: &mr_buffer_a[0] as *const u8 as u64,
        len: 128,
        key: mr_a.get_key(),
    };

    // let sge1 = Sge {
    //     addr: &mr_buffer_a[1] as *const u8 as u64,
    //     len: 1,
    //     key: mr_a.get_key(),
    // };

    // let sge2 = Sge {
    //     addr: &mr_buffer_a[2] as *const u8 as u64,
    //     len: 1,
    //     key: mr_a.get_key(),
    // };

    // let sge3 = Sge {
    //     addr: &mr_buffer_a[3] as *const u8 as u64,
    //     // len: 32767 - 3,
    //     len: SEND_CNT as u32 - 3,
    //     key: mr_a.get_key(),
    // };
    // let ctx = dev_a
    //     .write(
    //         &qp_b,
    //         &mr_buffer_b[65537] as *const u8 as u64,
    //         mr_b.get_key(),
    //         MemAccessTypeFlag::IbvAccessRemoteRead
    //             | MemAccessTypeFlag::IbvAccessRemoteWrite
    //             | MemAccessTypeFlag::IbvAccessLocalWrite,
    //         sge0,
    //         None,
    //         None,
    //         None,
    //         // Some(sge1),
    //         // Some(sge2),
    //         // Some(sge3),
    //     )
    //     .unwrap();
    // ctx.wait();
    // eprintln!("Write req sent");

    // for idx in 0..SEND_CNT {
    //     if mr_buffer_a[idx] != mr_buffer_b[65537 + idx] {
    //         let src_va = &mr_buffer_a[idx] as *const u8 as usize;
    //         let dst_va = &mr_buffer_b[65537 + idx] as *const u8 as usize;
    //         unsafe {
    //             println!(
    //                 "mr_buffer[{}]={}  != {}, src={:X}({:X}), dst={:X}({:X})",
    //                 idx,
    //                 mr_buffer_a[idx],
    //                 mr_buffer_b[65537 + idx],
    //                 src_va,
    //                 src_va - HEAP_START_ADDR,
    //                 dst_va,
    //                 dst_va - HEAP_START_ADDR
    //             );
    //         }
    //     }
    // }
    // assert!(mr_buffer_a[0..SEND_CNT] == mr_buffer_b[65537..65537 + SEND_CNT]);

    let sge_read = Sge {
        addr: &mr_buffer_b[0] as *const u8 as u64,
        len: 32767,
        // len: SEND_CNT as u32,
        key: mr_b.get_key(),
    };

    // read text from b to a.
    let ctx = dev_b
        .read(
            qp_a,
            &mr_buffer_a[0] as *const u8 as u64,
            mr_a.get_key(),
            MemAccessTypeFlag::IbvAccessNoFlags,
            sge_read,
        )
        .unwrap();
    ctx.wait();
    eprintln!("Read req sent");

    // // assert!(mr_buffer[0..0 + 32767] == mr_buffer[128 * 1024..128 * 1024 + 32767]);
    // assert!(mr_buffer_a[0..SEND_CNT] == mr_buffer_a[128 * 1024..128 * 1024 + SEND_CNT]);

    // dev_a.dereg_mr(mr_a).unwrap();
    // dev_b.dereg_mr(mr_b).unwrap();
    // eprintln!("MR deregistered");
}
