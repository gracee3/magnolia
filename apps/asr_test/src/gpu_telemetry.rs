use nvml_wrapper::Nvml;
use nvml_wrapper::enum_wrappers::device::{Clock, TemperatureSensor};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
struct GpuSample {
    util_gpu: Option<u32>,
    util_mem: Option<u32>,
    mem_used_mb: Option<u64>,
    mem_free_mb: Option<u64>,
    mem_total_mb: Option<u64>,
    proc_mem_mb: Option<u64>,
    temp_c: Option<u32>,
    power_w: Option<f32>,
    sm_clock_mhz: Option<u32>,
    mem_clock_mhz: Option<u32>,
}

#[derive(Debug)]
pub struct GpuTelemetry {
    nvml: Nvml,
    device_index: u32,
    pid: u32,
    sample_interval: Duration,
    last_sample: Instant,
    sample_count: u64,
    util_samples: u64,
    util_sum: u64,
    util_peak: u32,
    mem_util_peak: u32,
    mem_used_start_mb: Option<u64>,
    mem_used_end_mb: Option<u64>,
    mem_used_peak_mb: u64,
    proc_mem_peak_mb: u64,
    temp_peak_c: u32,
    power_peak_w: f32,
    sm_clock_peak_mhz: u32,
    mem_clock_peak_mhz: u32,
}

impl GpuTelemetry {
    pub fn new_if_enabled(device_index: u32) -> Option<Self> {
        if !env_bool("PARAKEET_GPU_TELEMETRY", false) {
            return None;
        }
        let nvml = Nvml::init().ok()?;
        let device_index =
            env_u64("PARAKEET_GPU_TELEMETRY_DEVICE", device_index as u64) as u32;
        let hz = env_u64("PARAKEET_GPU_TELEMETRY_HZ", 5).max(1);
        let interval_ms = (1000 / hz).max(1);
        let sample_interval = Duration::from_millis(interval_ms);
        let last_sample = Instant::now()
            .checked_sub(sample_interval)
            .unwrap_or_else(Instant::now);

        Some(Self {
            nvml,
            device_index,
            pid: std::process::id(),
            sample_interval,
            last_sample,
            sample_count: 0,
            util_samples: 0,
            util_sum: 0,
            util_peak: 0,
            mem_util_peak: 0,
            mem_used_start_mb: None,
            mem_used_end_mb: None,
            mem_used_peak_mb: 0,
            proc_mem_peak_mb: 0,
            temp_peak_c: 0,
            power_peak_w: 0.0,
            sm_clock_peak_mhz: 0,
            mem_clock_peak_mhz: 0,
        })
    }

    pub fn maybe_sample(&mut self) {
        if self.last_sample.elapsed() >= self.sample_interval {
            let _ = self.sample_now();
        }
    }

    pub fn mark_stage(&mut self, id: &str, utterance_seq: u64, stage: &str, extra: Option<&str>) {
        let Some(sample) = self.sample_now() else { return; };
        let extra = extra.unwrap_or("");
        let (extra_sep, extra) = if extra.is_empty() { ("", "") } else { (" ", extra) };
        eprintln!(
            "[asr_test] gpu_stage id={} utt_seq={} stage={} util_gpu={} util_mem={} mem_used_mb={} mem_free_mb={} mem_total_mb={} proc_mem_mb={} temp_c={} power_w={} sm_clock_mhz={} mem_clock_mhz={}{}{}",
            id,
            utterance_seq,
            stage,
            fmt_opt_u32(sample.util_gpu),
            fmt_opt_u32(sample.util_mem),
            fmt_opt_u64(sample.mem_used_mb),
            fmt_opt_u64(sample.mem_free_mb),
            fmt_opt_u64(sample.mem_total_mb),
            fmt_opt_u64(sample.proc_mem_mb),
            fmt_opt_u32(sample.temp_c),
            fmt_opt_f32(sample.power_w, 1),
            fmt_opt_u32(sample.sm_clock_mhz),
            fmt_opt_u32(sample.mem_clock_mhz),
            extra_sep,
            extra
        );
    }

    pub fn finish(&mut self, id: &str, utterance_seq: u64, pass_label: &str) {
        let _ = self.sample_now();
        let util_avg = if self.util_samples > 0 {
            Some(self.util_sum as f32 / self.util_samples as f32)
        } else {
            None
        };
        eprintln!(
            "[asr_test] gpu_summary id={} utt_seq={} pass={} samples={} util_avg={} util_peak={} mem_used_start_mb={} mem_used_peak_mb={} mem_used_end_mb={} proc_mem_peak_mb={} mem_util_peak={} temp_peak_c={} power_peak_w={} sm_clock_peak_mhz={} mem_clock_peak_mhz={}",
            id,
            utterance_seq,
            pass_label,
            self.sample_count,
            fmt_opt_f32(util_avg, 1),
            self.util_peak,
            fmt_opt_u64(self.mem_used_start_mb),
            self.mem_used_peak_mb,
            fmt_opt_u64(self.mem_used_end_mb),
            self.proc_mem_peak_mb,
            self.mem_util_peak,
            self.temp_peak_c,
            fmt_f32(self.power_peak_w, 1),
            self.sm_clock_peak_mhz,
            self.mem_clock_peak_mhz
        );
    }

    fn sample_now(&mut self) -> Option<GpuSample> {
        let device = self.nvml.device_by_index(self.device_index).ok()?;
        let util = device.utilization_rates().ok();
        let mem = device.memory_info().ok();
        let temp = device.temperature(TemperatureSensor::Gpu).ok();
        let power_mw = device.power_usage().ok();
        let sm_clock = device.clock_info(Clock::SM).ok();
        let mem_clock = device.clock_info(Clock::Memory).ok();
        let proc_mem_mb = process_mem_mb(&device, self.pid);

        let sample = GpuSample {
            util_gpu: util.as_ref().map(|u| u.gpu),
            util_mem: util.as_ref().map(|u| u.memory),
            mem_used_mb: mem.as_ref().map(|m| bytes_to_mb(m.used)),
            mem_free_mb: mem.as_ref().map(|m| bytes_to_mb(m.free)),
            mem_total_mb: mem.as_ref().map(|m| bytes_to_mb(m.total)),
            proc_mem_mb,
            temp_c: temp,
            power_w: power_mw.map(|mw| mw as f32 / 1000.0),
            sm_clock_mhz: sm_clock,
            mem_clock_mhz: mem_clock,
        };
        self.update_aggregate(&sample);
        self.last_sample = Instant::now();
        self.sample_count = self.sample_count.saturating_add(1);
        Some(sample)
    }

    fn update_aggregate(&mut self, sample: &GpuSample) {
        if let Some(util) = sample.util_gpu {
            self.util_sum = self.util_sum.saturating_add(util as u64);
            self.util_samples = self.util_samples.saturating_add(1);
            self.util_peak = self.util_peak.max(util);
        }
        if let Some(util) = sample.util_mem {
            self.mem_util_peak = self.mem_util_peak.max(util);
        }
        if let Some(mem_used) = sample.mem_used_mb {
            if self.mem_used_start_mb.is_none() {
                self.mem_used_start_mb = Some(mem_used);
            }
            self.mem_used_end_mb = Some(mem_used);
            self.mem_used_peak_mb = self.mem_used_peak_mb.max(mem_used);
        }
        if let Some(proc_mem) = sample.proc_mem_mb {
            self.proc_mem_peak_mb = self.proc_mem_peak_mb.max(proc_mem);
        }
        if let Some(temp) = sample.temp_c {
            self.temp_peak_c = self.temp_peak_c.max(temp);
        }
        if let Some(power) = sample.power_w {
            self.power_peak_w = self.power_peak_w.max(power);
        }
        if let Some(clock) = sample.sm_clock_mhz {
            self.sm_clock_peak_mhz = self.sm_clock_peak_mhz.max(clock);
        }
        if let Some(clock) = sample.mem_clock_mhz {
            self.mem_clock_peak_mhz = self.mem_clock_peak_mhz.max(clock);
        }
    }
}

fn process_mem_mb(
    device: &nvml_wrapper::Device,
    pid: u32,
) -> Option<u64> {
    use nvml_wrapper::enums::device::UsedGpuMemory;
    let mut total_bytes = 0u64;
    let mut found = false;
    if let Ok(procs) = device.running_compute_processes() {
        for p in procs {
            if p.pid != pid {
                continue;
            }
            if let UsedGpuMemory::Used(bytes) = p.used_gpu_memory {
                if bytes > 0 {
                    total_bytes = total_bytes.saturating_add(bytes);
                    found = true;
                }
            }
        }
    } else if let Ok(procs) = device.running_graphics_processes() {
        for p in procs {
            if p.pid != pid {
                continue;
            }
            if let UsedGpuMemory::Used(bytes) = p.used_gpu_memory {
                if bytes > 0 {
                    total_bytes = total_bytes.saturating_add(bytes);
                    found = true;
                }
            }
        }
    }
    if found {
        Some(bytes_to_mb(total_bytes))
    } else {
        None
    }
}

fn bytes_to_mb(bytes: u64) -> u64 {
    bytes / (1024 * 1024)
}

fn env_bool(name: &str, default: bool) -> bool {
    match std::env::var(name).ok().as_deref() {
        Some("1") | Some("true") | Some("yes") | Some("on") => true,
        Some("0") | Some("false") | Some("no") | Some("off") => false,
        Some(_) => default,
        None => default,
    }
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(default)
}

fn fmt_opt_u64(v: Option<u64>) -> String {
    v.map(|v| v.to_string()).unwrap_or_else(|| "-".to_string())
}

fn fmt_opt_u32(v: Option<u32>) -> String {
    v.map(|v| v.to_string()).unwrap_or_else(|| "-".to_string())
}

fn fmt_opt_f32(v: Option<f32>, precision: usize) -> String {
    v.map(|v| fmt_f32(v, precision))
        .unwrap_or_else(|| "-".to_string())
}

fn fmt_f32(v: f32, precision: usize) -> String {
    format!("{:.*}", precision, v)
}
