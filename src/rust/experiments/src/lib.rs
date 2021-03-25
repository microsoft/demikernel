use histogram::Histogram;
use anyhow::{format_err, Error};
use catnip::{
    logging,
    protocols::{
        ethernet2::MacAddress,
        ipv4::Endpoint,
        ip::Port,
    },
};
use std::time::Instant;
use catnip_libos::runtime::DPDKRuntime;
use std::str::FromStr;
use std::convert::TryFrom;
use std::env;
use std::ffi::CString;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::time::Duration;
use std::net::Ipv4Addr;
use yaml_rust::{
    Yaml,
    YamlLoader,
};

pub struct Latency {
    name: &'static str,
    samples: Vec<Duration>,
}

pub struct Recorder<'a> {
    start: Instant,
    latency: &'a mut Latency,
}

impl<'a> Drop for Recorder<'a> {
    fn drop(&mut self) {
        self.latency.samples.push(self.start.elapsed());
    }
}

impl Latency {
    pub fn new(name: &'static str, n: usize) -> Self {
        Self { name, samples: Vec::with_capacity(n) }
    }

    pub fn record(&mut self) -> Recorder<'_> {
        Recorder { start: Instant::now(), latency: self }
    }

    pub fn print(self) {
        println!("{} histogram", self.name);
        let mut h = Histogram::configure().precision(4).build().unwrap();

        // Drop the first 10% of samples.
        for s in &self.samples[(self.samples.len() / 10)..] {
            h.increment(s.as_nanos() as u64).unwrap();
        }
        print_histogram(&h);
    }

}

pub fn print_histogram(h: &Histogram) {
    println!(
        "p25:   {:?}",
        Duration::from_nanos(h.percentile(0.25).unwrap())
    );
    println!(
        "p50:   {:?}",
        Duration::from_nanos(h.percentile(0.50).unwrap())
    );
    println!(
        "p75:   {:?}",
        Duration::from_nanos(h.percentile(0.75).unwrap())
    );
    println!(
        "p90:   {:?}",
        Duration::from_nanos(h.percentile(0.90).unwrap())
    );
    println!(
        "p95:   {:?}",
        Duration::from_nanos(h.percentile(0.95).unwrap())
    );
    println!(
        "p99:   {:?}",
        Duration::from_nanos(h.percentile(0.99).unwrap())
    );
    println!(
        "p99.9: {:?}",
        Duration::from_nanos(h.percentile(0.999).unwrap())
    );
    println!("Min:   {:?}", Duration::from_nanos(h.minimum().unwrap()));
    println!("Avg:   {:?}", Duration::from_nanos(h.mean().unwrap()));
    println!("Max:   {:?}", Duration::from_nanos(h.maximum().unwrap()));
    println!("Stdev: {:?}", Duration::from_nanos(h.stddev().unwrap()));
}

#[derive(Debug)]
pub enum Experiment {
    Finite {
        num_iters: usize,
    },
    Continuous,
}

pub struct ExperimentStats {
    start: Instant,
    num_bytes: usize,
}

impl ExperimentStats {
    pub fn report_bytes(&mut self, n: usize) {
        self.num_bytes += n;
    }
}

impl Experiment {
    pub fn run(&self, mut f: impl FnMut(&mut ExperimentStats)) {
        let mut stats = ExperimentStats {
            start: Instant::now(),
            num_bytes: 0,
        };
        match self {
            Experiment::Finite { num_iters } => {
                let mut latency = Latency::new("round", *num_iters);
                for _ in 0..*num_iters {
                    let _s = latency.record();
                    f(&mut stats);
                }
                let exp_duration = stats.start.elapsed();
                if stats.num_bytes > 0 {
                    let throughput = Self::throughput_gbps(*num_iters, stats.num_bytes, exp_duration);
                    println!("Finished ({} samples, {} Gbps)", num_iters, throughput);
                }
            },
            Experiment::Continuous => {
                let mut last_log = Instant::now();
                for i in 0.. {
                    f(&mut stats);
                    if last_log.elapsed() > Duration::from_secs(2) {
                        last_log = Instant::now();
                        let throughput = Self::throughput_gbps(i, stats.num_bytes, stats.start.elapsed());
                        println!("Throughput: {} Gbps", throughput);
                    }
                }
            },
        }
    }

    fn throughput_gbps(num_iters: usize, num_bytes: usize, duration: Duration) -> f64 {
        let bps = (num_iters as f64 * num_bytes as f64) / duration.as_secs_f64();
        bps / 1024. / 1024. / 1024. * 8.
    }
}

#[derive(Debug)]
pub struct ExperimentConfig {
    pub buffer_size: usize,
    pub experiment: Experiment,
    pub config_obj: Yaml,
    pub mss: usize,
    pub strict: bool,
    pub udp_checksum_offload: bool,
    pub tcp_checksum_offload: bool,
}

impl ExperimentConfig {
    pub fn initialize() -> Result<(Self, DPDKRuntime), Error> {
        let config_path = env::args().nth(1).ok_or(format_err!("Config path is first argument"))?;
        let mut config_s = String::new();
        File::open(config_path)?.read_to_string(&mut config_s)?;
        let config = YamlLoader::load_from_str(&config_s)?;
        let config_obj = match &config[..] {
            &[ref c] => c,
            _ => Err(format_err!("Wrong number of config objects"))?,
        };
        let local_ipv4_addr: Ipv4Addr = config_obj["catnip"]["my_ipv4_addr"]
            .as_str()
            .ok_or_else(|| format_err!("Couldn't find my_ipv4_addr in config"))?
            .parse()?;
        if local_ipv4_addr.is_unspecified() || local_ipv4_addr.is_broadcast() {
            Err(format_err!("Invalid IPv4 address"))?;
        }

        let mut arp_table = HashMap::new();
        if let Some(arp_table_obj) = config_obj["catnip"]["arp_table"].as_hash() {
            for (k, v) in arp_table_obj {
                let key_str = k.as_str()
                    .ok_or_else(|| format_err!("Couldn't find ARP table key in config"))?;
                let key = MacAddress::parse_str(key_str)?;
                let value: Ipv4Addr = v.as_str()
                    .ok_or_else(|| format_err!("Couldn't find ARP table key in config"))?
                    .parse()?;
                arp_table.insert(key, value);
            }
        }
        let mut disable_arp = false;
        if let Some(arp_disabled) = config_obj["catnip"]["disable_arp"].as_bool() {
            disable_arp = arp_disabled;
        }

        let eal_init_args = match config_obj["dpdk"]["eal_init"] {
            Yaml::Array(ref arr) => arr
                .iter()
                .map(|a| {
                    a.as_str()
                        .ok_or_else(|| format_err!("Non string argument"))
                        .and_then(|s| CString::new(s).map_err(|e| e.into()))
                })
                .collect::<Result<Vec<_>, Error>>()?,
            _ => Err(format_err!("Malformed YAML config"))?,
        };

        let use_jumbo_frames = env::var("USE_JUMBO").is_ok();
        let mtu: u16 = env::var("MTU")?.parse()?;
        let mss: usize = env::var("MSS")?.parse()?;
        let tcp_checksum_offload = env::var("TCP_CHECKSUM_OFFLOAD").is_ok();
        let udp_checksum_offload = env::var("UDP_CHECKSUM_OFFLOAD").is_ok();
        let strict = env::var("STRICT").is_ok();

        let buffer_size: usize = env::var("BUFFER_SIZE")?.parse()?;

        let experiment = if env::var("CONTINUOUS").is_ok() {
            Experiment::Continuous
        } else {
            let num_iters = env::var("NUM_ITERS")?.parse()?;
            Experiment::Finite { num_iters }
        };

        let runtime = catnip_libos::dpdk::initialize_dpdk(
            local_ipv4_addr,
            &eal_init_args,
            arp_table,
            disable_arp,
            use_jumbo_frames,
            mtu,
            mss,
            tcp_checksum_offload,
            udp_checksum_offload,
        )?;
        logging::initialize();

        let config = Self {
            buffer_size,
            experiment,
            mss,
            strict,
            tcp_checksum_offload,
            udp_checksum_offload,
            config_obj: config_obj.clone(),
        };
        Ok((config, runtime))
    }

    pub fn addr(&self, k1: &str, k2: &str) -> Result<Endpoint, Error> {
        let addr = &self.config_obj[k1][k2];
        let host_s = addr["host"].as_str().ok_or(format_err!("Missing host"))?;
        let host = Ipv4Addr::from_str(host_s)?;
        let port_i = addr["port"].as_i64().ok_or(format_err!("Missing port"))?;
        let port = Port::try_from(port_i as u16)?;
        Ok(Endpoint::new(host, port))
    }
}
