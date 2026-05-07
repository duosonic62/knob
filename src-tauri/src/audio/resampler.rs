use std::collections::VecDeque;

use rubato::{
    Resampler, SincFixedOut, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};

pub struct StereoResampler {
    inner: SincFixedOut<f32>,
    in_buf: Vec<Vec<f32>>,  // [2][input_frames_max] per-channel
    in_scratch: Vec<f32>,   // interleaved pop scratch for one input chunk
    out_buf: Vec<Vec<f32>>, // [2][output_chunk] per-channel
    fifo: VecDeque<f32>,    // interleaved output FIFO (single-threaded)
}

impl StereoResampler {
    pub fn new(input_rate: f64, output_rate: f64, output_chunk: usize) -> Result<Self, String> {
        let params = SincInterpolationParameters {
            sinc_len: 64,
            f_cutoff: 0.95,
            interpolation: SincInterpolationType::Linear,
            oversampling_factor: 128,
            window: WindowFunction::BlackmanHarris2,
        };
        let inner = SincFixedOut::<f32>::new(
            output_rate / input_rate,
            2.0,
            params,
            output_chunk,
            2,
        )
        .map_err(|e| e.to_string())?;

        let max_in = inner.input_frames_max();
        let in_buf = vec![vec![0f32; max_in]; 2];
        let in_scratch = vec![0f32; max_in * 2];
        let out_buf = vec![vec![0f32; output_chunk]; 2];
        let fifo = VecDeque::with_capacity(output_chunk * 6);

        Ok(Self { inner, in_buf, in_scratch, out_buf, fifo })
    }

    /// Fill `output` (interleaved stereo, `frame_count * 2` samples) by resampling
    /// from the ring consumer. xruns → silence; errors → output left at zero.
    pub fn process(&mut self, output: &mut [f32], input: &mut ringbuf::HeapCons<f32>) {
        let required = output.len();

        while self.fifo.len() < required {
            let needed_in = self.inner.input_frames_next();
            let pop_len = needed_in * 2;

            // Zero the input scratch so xrun frames are silence
            self.in_scratch[..pop_len].fill(0.0);
            ringbuf::traits::Consumer::pop_slice(input, &mut self.in_scratch[..pop_len]);

            // De-interleave into per-channel input buffers
            for i in 0..needed_in {
                self.in_buf[0][i] = self.in_scratch[i * 2];
                self.in_buf[1][i] = self.in_scratch[i * 2 + 1];
            }

            let (_, n_out) = match self.inner.process_into_buffer(
                &self.in_buf,
                &mut self.out_buf,
                None,
            ) {
                Ok(v) => v,
                Err(_) => return, // output stays zero (caller zero-filled the buffer)
            };

            // Re-interleave and push to FIFO
            for i in 0..n_out {
                self.fifo.push_back(self.out_buf[0][i]);
                self.fifo.push_back(self.out_buf[1][i]);
            }
        }

        for slot in output.iter_mut() {
            *slot = self.fifo.pop_front().unwrap_or(0.0);
        }
    }
}
