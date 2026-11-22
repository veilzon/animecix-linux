use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SizedSample};

use super::session::Command;

const RATE: u32 = 16_000;
const FRAME_SAMPLES: usize = 320; // 20 ms

struct OutputState {
    queue: VecDeque<i16>,
    phase: f64,
}

/// Bas-konuş ses motoru. Giriş yalnızca PTT basılıyken çerçeve üretir;
/// çıkış gelen çerçeveleri sırayla çalar. Cihaz yoksa `Err` döner.
pub struct AudioEngine {
    _input: cpal::Stream,
    _output: cpal::Stream,
    ptt: Arc<AtomicBool>,
    out: Arc<Mutex<OutputState>>,
}

impl AudioEngine {
    pub fn start(cmd_tx: mpsc::Sender<Command>) -> Result<Self, String> {
        let host = cpal::default_host();
        let input_device = host
            .default_input_device()
            .ok_or_else(|| "mikrofon bulunamadı".to_string())?;
        let output_device = host
            .default_output_device()
            .ok_or_else(|| "ses çıkışı bulunamadı".to_string())?;

        let in_cfg = input_device
            .default_input_config()
            .map_err(|e| format!("mikrofon ayarı: {e}"))?;
        let out_cfg = output_device
            .default_output_config()
            .map_err(|e| format!("hoparlör ayarı: {e}"))?;
        let in_rate = in_cfg.sample_rate().0;
        let out_rate = out_cfg.sample_rate().0;
        let in_channels = in_cfg.channels() as usize;
        let out_channels = out_cfg.channels() as usize;

        let ptt = Arc::new(AtomicBool::new(false));
        let capture = Arc::new(Mutex::new(Vec::<f32>::new()));
        let out = Arc::new(Mutex::new(OutputState {
            queue: VecDeque::new(),
            phase: 0.0,
        }));

        let input = match in_cfg.sample_format() {
            cpal::SampleFormat::I16 => build_input::<i16>(
                &input_device,
                &in_cfg.into(),
                in_channels,
                in_rate,
                ptt.clone(),
                capture.clone(),
                cmd_tx,
            )?,
            cpal::SampleFormat::U16 => build_input::<u16>(
                &input_device,
                &in_cfg.into(),
                in_channels,
                in_rate,
                ptt.clone(),
                capture.clone(),
                cmd_tx,
            )?,
            cpal::SampleFormat::F32 => build_input::<f32>(
                &input_device,
                &in_cfg.into(),
                in_channels,
                in_rate,
                ptt.clone(),
                capture.clone(),
                cmd_tx,
            )?,
            other => return Err(format!("desteklenmeyen mikrofon biçimi: {other:?}")),
        };
        input
            .play()
            .map_err(|e| format!("mikrofon başlatılamadı: {e}"))?;

        let output = match out_cfg.sample_format() {
            cpal::SampleFormat::I16 => build_output::<i16>(
                &output_device,
                &out_cfg.into(),
                out_channels,
                out_rate,
                out.clone(),
            )?,
            cpal::SampleFormat::U16 => build_output::<u16>(
                &output_device,
                &out_cfg.into(),
                out_channels,
                out_rate,
                out.clone(),
            )?,
            cpal::SampleFormat::F32 => build_output::<f32>(
                &output_device,
                &out_cfg.into(),
                out_channels,
                out_rate,
                out.clone(),
            )?,
            other => return Err(format!("desteklenmeyen hoparlör biçimi: {other:?}")),
        };
        output
            .play()
            .map_err(|e| format!("hoparlör başlatılamadı: {e}"))?;

        Ok(Self {
            _input: input,
            _output: output,
            ptt,
            out,
        })
    }

    pub fn set_ptt(&self, held: bool) {
        self.ptt.store(held, Ordering::Relaxed);
    }

    pub fn play(&self, samples: &[i16]) {
        if samples.is_empty() {
            return;
        }
        if let Ok(mut out) = self.out.lock() {
            out.queue.extend(samples.iter().copied());
            // Gecikmeyi sınırla: 400 ms'den fazla biriktirme.
            let max = (RATE as usize / 1000) * 400;
            while out.queue.len() > max {
                out.queue.pop_front();
            }
        }
    }
}

fn build_input<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    rate: u32,
    ptt: Arc<AtomicBool>,
    capture: Arc<Mutex<Vec<f32>>>,
    cmd_tx: mpsc::Sender<Command>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32> + Sample,
    f32: FromSample<T>,
{
    let channels = channels.max(1);
    let step = (rate as f64 / RATE as f64).max(1.0);
    let mut acc = 0.0f64;
    let err_fn = |e| eprintln!("[WATCHPARTY] mikrofon: {e}");
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                if !ptt.load(Ordering::Relaxed) {
                    return;
                }
                let Ok(mut buf) = capture.lock() else {
                    return;
                };
                for frame in data.chunks(channels) {
                    let mono =
                        frame.iter().map(|s| s.to_sample::<f32>()).sum::<f32>() / channels as f32;
                    acc += 1.0;
                    if acc >= step {
                        acc -= step;
                        buf.push(mono.clamp(-1.0, 1.0));
                    }
                }
                while buf.len() >= FRAME_SAMPLES {
                    let samples: Vec<i16> = buf
                        .drain(0..FRAME_SAMPLES)
                        .map(|s| (s * i16::MAX as f32) as i16)
                        .collect();
                    let _ = cmd_tx.send(Command::Voice(samples));
                }
            },
            err_fn,
            None,
        )
        .map_err(|e| format!("mikrofon akışı: {e}"))
}

fn build_output<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    rate: u32,
    out: Arc<Mutex<OutputState>>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32> + Sample,
{
    let channels = channels.max(1);
    let step = RATE as f64 / rate as f64;
    let err_fn = |e| eprintln!("[WATCHPARTY] hoparlör: {e}");
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                let Ok(mut st) = out.lock() else {
                    for s in data.iter_mut() {
                        *s = T::from_sample(0.0);
                    }
                    return;
                };
                for frame in data.chunks_mut(channels) {
                    st.phase += step;
                    while st.phase >= 1.0 {
                        st.phase -= 1.0;
                        st.queue.pop_front();
                    }
                    let sample = st.queue.front().copied().unwrap_or(0) as f32 / i16::MAX as f32;
                    let v = T::from_sample(sample);
                    for s in frame.iter_mut() {
                        *s = v;
                    }
                }
            },
            err_fn,
            None,
        )
        .map_err(|e| format!("hoparlör akışı: {e}"))
}
