use std::net::Ipv4Addr;
use std::convert::TryFrom;
use std::{thread, time};

use aya::{
    include_bytes_aligned,
    maps::{RingBuf, HashMap},
    programs::{CgroupAttachMode, CgroupSkb, CgroupSkbAttachType},
    util::online_cpus,
    Ebpf,
};
use bytes::BytesMut;
use clap::Parser;
use log::info;
use tokio::{signal, task};

use cgroup_skb_egress_common::PacketLog;

//  TODO: Include ebpf version, do not duplicate code.
#[derive(Copy, Clone)]
pub struct EbpfAtomicI64 {
    value: i64,
    zero: i64,
}

impl Default for EbpfAtomicI64 {
    fn default() -> EbpfAtomicI64 {
        EbpfAtomicI64 {
            value: 0,
            zero: 0,
        }
    }
}

unsafe impl aya::Pod for EbpfAtomicI64 {}

#[derive(Debug, Parser)]
struct Opt {
    #[clap(short, long, default_value = "/sys/fs/cgroup/aya-book-cgroup-skb-egress")]
    cgroup_path: String,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let opt = Opt::parse();

    // env_logger::init();

    // This will include your eBPF object file as raw bytes at compile-time and load it at
    // runtime. This approach is recommended for most real-world use cases. If you would
    // like to specify the eBPF program at runtime rather than at compile-time, you can
    // reach for `Ebpf::load_file` instead.
    #[cfg(debug_assertions)]
    let mut bpf = Ebpf::load(include_bytes_aligned!(
        "../../target/bpfel-unknown-none/debug/cgroup-skb-egress"
    ))?;
    #[cfg(not(debug_assertions))]
    let mut bpf = Ebpf::load(include_bytes_aligned!(
        "../../target/bpfel-unknown-none/release/cgroup-skb-egress"
    ))?;
    let program: &mut CgroupSkb =
        bpf.program_mut("cgroup_skb_egress").unwrap().try_into()?;
    let cgroup = std::fs::File::open(opt.cgroup_path)?;
    // (1)
    program.load()?;
    // (2)
    program.attach(
        cgroup,
        CgroupSkbAttachType::Egress,
        CgroupAttachMode::Single,
    )?;

    let mut blocklist: HashMap<_, u32, EbpfAtomicI64> =
        HashMap::try_from(bpf.map_mut("BLOCKLIST").unwrap())?;

    let block_addr: u32 = Ipv4Addr::new(1, 1, 1, 1).try_into()?;

    // (3)
    blocklist.insert(block_addr, EbpfAtomicI64::default(), 0)?;

    // let mut perf_array =
    //     AsyncPerfEventArray::try_from(bpf.take_map("EVENTS").unwrap())?;
    let mut perf_array = RingBuf::try_from(bpf.map_mut("EVENTS").unwrap()).unwrap();

    info!("Waiting for Ctrl-C...");

    loop {
        thread::sleep(time::Duration::from_millis(1000));

        while let Some(item) = perf_array.next() {
            println!("Received: {:?}", item);
            let ptr = item.as_ptr() as *const PacketLog;
            let data = unsafe { ptr.read_unaligned() };
            let src_addr = Ipv4Addr::from(data.ipv4_address);
            info!("LOG: DST {}, ACTION {}", src_addr, data.action);
        }
    }

    // for cpu_id in online_cpus().map_err(|(_, error)| error)? {
    //     let mut buf = perf_array.open(cpu_id, None)?;

    //     task::spawn(async move {
    //         let mut buffers = (0..10)
    //             .map(|_| BytesMut::with_capacity(1024))
    //             .collect::<Vec<_>>();

    //         loop {
    //             let events = buf.read_events(&mut buffers).await.unwrap();
    //             for buf in buffers.iter_mut().take(events.read) {
    //             }
    //         }
    //     });
    // }

}
