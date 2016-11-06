use ::rand;

#[derive(Serialize, Deserialize, Clone)]
pub struct Network {
  pub layer_sizes: Vec<usize>,
  pub activation_coeffs: Vec<f32>,
  pub weights: Vec<Vec<f32>>,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct TrainConfig {
  pub learning_rate: f32,
  pub momentum_rate: f32,
  pub validation_ratio: f32,
  pub sequential_validation_failures_required: usize,
  pub max_epochs: Option<usize>,
}

pub type TrainData = Vec<(Vec<f32>, Vec<f32>)>;

impl Network {
  pub fn from_definition(layer_sizes: Vec<usize>, activation_coeffs: Vec<f32>) -> Network {
    Network {
      layer_sizes: layer_sizes.clone(),
      activation_coeffs: activation_coeffs,
      weights: layer_sizes.windows(2).map(|w| {
        let mut v = Vec::with_capacity(w[0] * w[1]);
        unsafe { v.set_len(w[0] * w[1]) };
        v
      }).collect::<Vec<_>>(),
    }
  }

  pub fn assign_random_weights<R: rand::Rng>(&mut self, rng: &mut R) {
    use rand::distributions::{Normal, IndependentSample};

    let weight_dist = Normal::new(0., 1.);
    for matrix in &mut self.weights {
      for weight in matrix.iter_mut() {
        *weight = weight_dist.ind_sample(rng) as f32;
      }
    }
  }

  pub fn train(&mut self, train_data: TrainData, validation_data: TrainData, conf: &TrainConfig) {
    let mut epochs_since_validation_improvement = 0usize;
    let mut layers = (0..(self.layer_sizes.len())).map(|it| {
      let mut v = Vec::with_capacity(self.layer_sizes[it]);
      unsafe { v.set_len(self.layer_sizes[it]); }
      v
    }).collect::<Vec<_>>();
    let mut delta = layers.clone();

    while epochs_since_validation_improvement < conf.sequential_validation_failures_required {
      let mut train_error = 0.0;
      let mut validation_error = ::std::f32::INFINITY;

      for &(ref input, ref output) in &train_data {
        layers[0] = input.clone();
        self.feed_forward(&mut layers);
        let mut out_layer_err = layers.last().unwrap()
          .iter()
          .zip(output)
          .map(|(y, o)| (o - y)*(o - y))
          .collect::<Vec<_>>();
        
        train_error += out_layer_err.iter().sum::<f32>() / out_layer_err.len() as f32;
        self.backpropagate(layers.clone(), &out_layer_err[..], &mut delta, conf);
        self.update_weights(&layers, &delta, conf);
      }
      
      let new_validation_error = validation_data.iter()
        .map(|ex| self.validation_error_of(&mut layers, &ex.0[..], &ex.1[..])).sum::<f32>();
      if new_validation_error > validation_error {
        epochs_since_validation_improvement += 1;
      } else {
        epochs_since_validation_improvement = 0;
      }
      validation_error = new_validation_error;
    }
  }

  fn validation_error_of(&self, layers: &mut Vec<Vec<f32>>, example: &[f32], target: &[f32]) -> f32 {
    assert_eq!(layers[0].len(), example.len());
    assert_eq!(layers.last().unwrap().len(), target.len());
    for (x, ex) in layers[0].iter_mut().zip(example) {
      *x = *ex;
    }

    self.feed_forward(layers);
    let out_layer_err = layers.last().unwrap()
      .iter()
      .zip(layers.last().unwrap())
      .map(|(y, o)| (o - y)*(o - y))
      .collect::<Vec<_>>();
    out_layer_err.iter().sum::<f32>() / out_layer_err.len() as f32
  }

  fn feed_forward(&self, layers: &mut Vec<Vec<f32>>) {
    use ::mmul;
    for window in (0..layers.len()).collect::<Vec<_>>().windows(2) {
      let (it, jt) = (window[0], window[1]);
      // let (inl, mut outl) = (&layers[it], &mut layers[jt]);
      let inl_ptr = layers[it].as_ptr();
      let outl_ptr = layers[it].as_mut_ptr();
      unsafe {
        mmul::sgemm(
          1,
          layers[it].len(),
          layers[jt].len(),
          self.activation_coeffs[it],
          inl_ptr,
          1,
          1,
          self.weights[it].as_ptr(),
          1,
          1,
          0.0,
          outl_ptr,
          1,
          1
        );
      }
      for net in layers[jt].iter_mut() {
        *net = Network::sigmoid(*net);
      }
    }
  }

  fn backpropagate(&mut self, mut layers: Vec<Vec<f32>>, out_layer_err: &[f32], delta: &mut Vec<Vec<f32>>, conf: &TrainConfig) {
    use ::mmul;
    // NOTE(msniegocki): the initial net activation can be reused due
    // the useful derivative propperty of the sigmoid function
    for layer in layers.iter_mut() {
      for (out, coeff) in layer.iter_mut().zip(&self.activation_coeffs) {
        *out = *out * (1.0 - *out) * coeff;
      }
    }

    *delta.last_mut().unwrap() = out_layer_err
        .iter()
        .zip(layers.last().unwrap())
        .map(|(e, fz)| e * fz)
        .collect();
    for it in (0..(layers.len() - 1)).rev() {
      unsafe {
        mmul::sgemm(
          1,
          delta[it+1].len(),
          layers[it].len(),
          1.0,
          delta[it+1].as_ptr(),
          1,
          1,
          self.weights[it].as_ptr(),
          1,
          1,
          0.0,
          delta[it].as_mut_ptr(),
          1,
          1
        );
      }
    }
  }

  fn update_weights(&mut self, layers: &Vec<Vec<f32>>, delta: &Vec<Vec<f32>>, conf: &TrainConfig) {
    use ::mmul;

    for it in 0..(layers.len() - 1) {
      assert_eq!(self.weights[it].len(), delta[it].len() * layers[it].len());
      unsafe {
        mmul::sgemm(
          delta[it].len(),
          1,
          layers[it+1].len(),
          1.0,
          delta[it].as_ptr(),
          1,
          1,
          layers[it+1].as_ptr(),
          1,
          1,
          conf.learning_rate,
          self.weights[it].as_mut_ptr(),
          1,
          1
        );
      }
    }
  }

  fn sigmoid(t: f32) -> f32 {
    1.0 / (1.0 + (-t).exp())
  }

  fn sigmoid_prime(t: f32) -> f32 {
    Network::sigmoid(t) * (1.0 - Network::sigmoid(t))
  }

  pub fn write(&self, filename: &str) {
    use ::{std, bc};
    use std::error::Error;
    use std::fs::File;
    use std::io::Write;

    let bytes = bc::serde::serialize(self, bc::SizeLimit::Infinite)
      .unwrap_or_else(
        |err| {
          let _ = writeln!(
            std::io::stderr(),
            "bincode serialization error: {}\n{}",
            err.description(),
            err.cause().map(Error::description).unwrap_or(""));
        Vec::new()
      });
    
    let mut file: File = match File::create(filename) {
      Err(err) =>
        panic!(
          "failed to create file {}: {}\n{}",
          filename,
          err.description(),
          err.cause().map(Error::description).unwrap_or("")),
      Ok(f) => f,
    };

    file.write_all(&bytes).unwrap_or_else(
      |err| panic!(
        "failed to write to file {}: {}\n{}",
        filename,
        err.description(),
        err.cause().map(Error::description).unwrap_or("")));
  }
}