use rand::distributions::WeightedIndex;
use rand::{CryptoRng, Rng, RngCore};
use rand_distr::{num_traits::ToPrimitive, Distribution};
use std::time::Duration;

use crate::config::Config;

pub struct Sample {
    config: Config,

    // Event Count
    reach_per_ad_distr: WeightedIndex<f64>,
    cvr_per_adaccount_distr: WeightedIndex<f64>,
    ad_impression_per_user_distr: WeightedIndex<f64>,
    ad_conversion_per_user_distr: WeightedIndex<f64>,

    // Match key
    devices_per_user_distr: WeightedIndex<f64>,

    // Time
    conversions_duration_distr: WeightedIndex<f64>,
    frequency_cap_distr: WeightedIndex<f64>,

    // Trigger value
    trigger_value_distr: WeightedIndex<f64>,
}

impl Sample {
    // <# of events> = X = DEFAULT_EVENT_GEN_COUNT * scale
    // # of events per day = impressions/day + conversions/day
    // impressions per day = devices * impression/device/day
    pub fn new(config: &Config) -> Self {
        Self {
            config: config.clone(),
            reach_per_ad_distr: WeightedIndex::new(config.reach_per_ad().iter().map(|i| i.1))
                .unwrap(),
            cvr_per_adaccount_distr: WeightedIndex::new(config.cvr_per_ad().iter().map(|i| i.1))
                .unwrap(),
            ad_impression_per_user_distr: WeightedIndex::new(
                config.impression_per_user().iter().map(|i| i.1),
            )
            .unwrap(),
            ad_conversion_per_user_distr: WeightedIndex::new(
                config.conversion_per_user().iter().map(|i| i.1),
            )
            .unwrap(),

            devices_per_user_distr: WeightedIndex::new(
                config.devices_per_user().iter().map(|i| i.1),
            )
            .unwrap(),

            conversions_duration_distr: WeightedIndex::new(
                config.impression_conversion_duration().iter().map(|i| i.1),
            )
            .unwrap(),

            frequency_cap_distr: WeightedIndex::new(
                config.impression_impression_duration().iter().map(|i| i.1),
            )
            .unwrap(),

            // TODO: Need data
            trigger_value_distr: WeightedIndex::new(
                config.conversion_value_per_user().iter().map(|i| i.1),
            )
            .unwrap(),
        }
    }

    pub fn reach_per_ad<R: RngCore + CryptoRng>(&self, rng: &mut R) -> u32 {
        // TODO: Using impressions distribution here because 93% of users see only have one impression per ad
        let r = self.config.reach_per_ad()[self.reach_per_ad_distr.sample(rng)]
            .0
            .clone();
        rng.gen_range(r)
    }

    pub fn devices_per_user<R: RngCore + CryptoRng>(&self, rng: &mut R) -> u8 {
        self.config.devices_per_user()[self.devices_per_user_distr.sample(rng)].0
    }

    pub fn cvr_per_ad_account<R: RngCore + CryptoRng>(&self, rng: &mut R) -> f64 {
        let r = self.config.cvr_per_ad()[self.cvr_per_adaccount_distr.sample(rng)]
            .0
            .clone();
        rng.gen_range(r)
    }

    pub fn impression_per_user<R: RngCore + CryptoRng>(&self, rng: &mut R) -> u8 {
        self.config.impression_per_user()[self.ad_impression_per_user_distr.sample(rng)].0
    }

    pub fn conversion_per_user<R: RngCore + CryptoRng>(&self, rng: &mut R) -> u8 {
        self.config.conversion_per_user()[self.ad_conversion_per_user_distr.sample(rng)].0
    }

    pub fn conversion_value_per_ad<R: RngCore + CryptoRng>(&self, rng: &mut R) -> u32 {
        let r = self.config.conversion_value_per_user()[self.trigger_value_distr.sample(rng)]
            .0
            .clone();
        rng.gen_range(r)
    }

    pub fn impressions_time_diff<R: RngCore + CryptoRng>(&self, rng: &mut R) -> Duration {
        let r = self.config.impression_impression_duration()[self.frequency_cap_distr.sample(rng)]
            .0
            .clone();
        let diff = rng.gen_range(r);
        Duration::new((diff * 60.0 * 60.0).floor().to_u64().unwrap(), 0)
    }

    pub fn conversions_time_diff<R: RngCore + CryptoRng>(&self, rng: &mut R) -> Duration {
        let days = self.config.impression_conversion_duration()
            [self.conversions_duration_distr.sample(rng)]
        .0
        .clone();
        let diff = rng.gen_range(days);

        // Since [diff] is a range of days, randomly choose hours and seconds for the given range.
        // E.g. return [1..3) days + y hours + z seconds
        Duration::new(diff.to_u64().unwrap() * 24 * 60 * 60, 0)
            + Duration::new(rng.gen_range(0..23) * 60 * 60, 0)
            + Duration::new(rng.gen_range(0..59) * 60, 0)
            + Duration::new(rng.gen_range(0..59), 0)
    }
}
