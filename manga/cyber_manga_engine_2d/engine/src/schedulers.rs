use candle_core::{Result, Tensor};
use candle_transformers::models::stable_diffusion::schedulers::{
    BetaSchedule, PredictionType, Scheduler, SchedulerConfig, TimestepSpacing,
};

// Utils implementations
fn linspace(start: f64, end: f64, steps: usize) -> Vec<f64> {
    if steps == 0 {
        return vec![];
    }
    if steps == 1 {
        return vec![start];
    }
    let delta = (end - start) / (steps - 1) as f64;
    (0..steps).map(|i| start + delta * i as f64).collect()
}

fn interp(x: &[f64], xp: &[f64], fp: &[f64]) -> Vec<f64> {
    let mut ys = Vec::with_capacity(x.len());
    for &num in x {
        if num < xp[0] {
            ys.push(fp[0]);
            continue;
        }
        if num > *xp.last().unwrap() {
            ys.push(*fp.last().unwrap());
            continue;
        }
        let idx = xp.partition_point(|&v| v <= num);
        let idx = if idx == 0 { 0 } else { idx - 1 };

        let t = (num - xp[idx]) / (xp[idx + 1] - xp[idx]);
        ys.push(fp[idx] + t * (fp[idx + 1] - fp[idx]));
    }
    ys
}

#[derive(Debug, Clone, Copy)]
pub struct EulerDiscreteSchedulerConfig {
    pub beta_start: f64,
    pub beta_end: f64,
    pub beta_schedule: BetaSchedule,
    pub steps_offset: usize,
    pub prediction_type: PredictionType,
    pub train_timesteps: usize,
    pub timestep_spacing: TimestepSpacing,
}

impl Default for EulerDiscreteSchedulerConfig {
    fn default() -> Self {
        Self {
            beta_start: 0.00085f64,
            beta_end: 0.012f64,
            beta_schedule: BetaSchedule::ScaledLinear,
            steps_offset: 1,
            prediction_type: PredictionType::Epsilon,
            train_timesteps: 1000,
            timestep_spacing: TimestepSpacing::Leading,
        }
    }
}

impl SchedulerConfig for EulerDiscreteSchedulerConfig {
    fn build(&self, inference_steps: usize) -> Result<Box<dyn Scheduler>> {
        let mut scheduler = EulerDiscreteScheduler::new(*self)?;
        scheduler.set_timesteps(inference_steps)?;
        Ok(Box::new(scheduler))
    }
}

#[derive(Debug, Clone)]
pub struct EulerDiscreteScheduler {
    timesteps: Vec<usize>,
    sigmas: Vec<f64>,
    init_noise_sigma: f64,
    pub config: EulerDiscreteSchedulerConfig,
}

impl EulerDiscreteScheduler {
    pub fn new(config: EulerDiscreteSchedulerConfig) -> Result<Self> {
        let mut scheduler = Self {
            timesteps: vec![],
            sigmas: vec![],
            init_noise_sigma: 0.0,
            config,
        };
        // Initialize with default steps (50) to ensure valid state
        scheduler.set_timesteps(50)?;
        Ok(scheduler)
    }

    pub fn set_timesteps(&mut self, inference_steps: usize) -> Result<()> {
        let config = &self.config;
        let step_ratio = config.train_timesteps / inference_steps;
        let timesteps: Vec<usize> = match config.timestep_spacing {
            TimestepSpacing::Leading => (0..inference_steps)
                .map(|s| s * step_ratio + config.steps_offset)
                .rev()
                .collect(),
            TimestepSpacing::Trailing => std::iter::successors(Some(config.train_timesteps), |n| {
                if *n > step_ratio {
                    Some(n - step_ratio)
                } else {
                    None
                }
            })
            .map(|n| n - 1)
            .collect(),
            TimestepSpacing::Linspace => {
                let arr = linspace(0.0, (config.train_timesteps - 1) as f64, inference_steps);
                arr.iter().map(|&f| f as usize).rev().collect()
            }
        };

        let betas = match config.beta_schedule {
            BetaSchedule::ScaledLinear => {
                let s = config.beta_start.sqrt();
                let e = config.beta_end.sqrt();
                let b = linspace(s, e, config.train_timesteps);
                b.iter().map(|v| v * v).collect()
            }
            BetaSchedule::Linear => {
                linspace(config.beta_start, config.beta_end, config.train_timesteps)
            }
            BetaSchedule::SquaredcosCapV2 => {
                // betas_for_alpha_bar(config.train_timesteps, 0.999)?.to_vec1::<f64>()?
                candle_core::bail!("SquaredcosCapV2 not implemented")
            }
        };

        let mut alphas_cumprod = Vec::with_capacity(betas.len());
        let mut cur_alpha_cumprod = 1.0;
        for &beta in betas.iter() {
            let alpha = 1.0 - beta;
            cur_alpha_cumprod *= alpha;
            alphas_cumprod.push(cur_alpha_cumprod);
        }

        let sigmas: Vec<f64> = alphas_cumprod
            .iter()
            .map(|&f| ((1.0 - f) / f).sqrt())
            .collect();

        let sigmas_xa: Vec<f64> = (0..sigmas.len()).map(|i| i as f64).collect();
        let timesteps_float: Vec<f64> = timesteps.iter().map(|&t| t as f64).collect();

        let mut sigmas_int = interp(&timesteps_float, &sigmas_xa, &sigmas);
        sigmas_int.push(0.0);

        let init_noise_sigma = *sigmas_int
            .iter()
            .max_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap_or(&0.0);

        self.timesteps = timesteps;
        self.sigmas = sigmas_int;
        self.init_noise_sigma = init_noise_sigma;

        Ok(())
    }
}

impl Scheduler for EulerDiscreteScheduler {
    fn timesteps(&self) -> &[usize] {
        self.timesteps.as_slice()
    }

    fn scale_model_input(&self, sample: Tensor, timestep: usize) -> Result<Tensor> {
        let step_index = match self.timesteps.iter().position(|&t| t == timestep) {
            Some(i) => i,
            None => candle_core::bail!("timestep out of this schedulers bounds: {timestep}"),
        };

        let sigma = self.sigmas[step_index];
        sample / ((sigma.powi(2) + 1.0).sqrt())
    }

    fn step(&mut self, model_output: &Tensor, timestep: usize, sample: &Tensor) -> Result<Tensor> {
        let step_index = self
            .timesteps
            .iter()
            .position(|&p| p == timestep)
            .ok_or_else(|| {
                candle_core::Error::Msg("timestep out of this schedulers bounds".to_string())
            })?;

        let sigma_from = self.sigmas[step_index];
        let sigma_to = self.sigmas[step_index + 1];

        // 1. compute predicted original sample (x_0) from sigma-scaled predicted noise
        let pred_original_sample = match self.config.prediction_type {
            PredictionType::Epsilon => (sample - (model_output * sigma_from)?)?,
            PredictionType::VPrediction => {
                let sigma_from_sq = sigma_from.powi(2);
                let sigma_from_sq_plus_1_sqrt = (sigma_from_sq + 1.0).sqrt();
                ((model_output * (-sigma_from / sigma_from_sq_plus_1_sqrt))?
                    + (sample / (sigma_from_sq + 1.0))?)?
            }
            PredictionType::Sample => {
                candle_core::bail!("prediction_type not implemented yet: sample")
            }
        };

        let derivative = ((sample - pred_original_sample)? / sigma_from)?;
        let dt = sigma_to - sigma_from;

        sample + (derivative * dt)?
    }

    fn add_noise(&self, original: &Tensor, noise: Tensor, timestep: usize) -> Result<Tensor> {
        let step_index = self
            .timesteps
            .iter()
            .position(|&p| p == timestep)
            .ok_or_else(|| {
                candle_core::Error::Msg("timestep out of this schedulers bounds".to_string())
            })?;

        let sigma = self.sigmas[step_index];
        original + (noise * sigma)?
    }

    fn init_noise_sigma(&self) -> f64 {
        self.init_noise_sigma
    }
}
