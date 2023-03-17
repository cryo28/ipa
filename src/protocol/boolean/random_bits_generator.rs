use super::{
    solved_bits::{solved_bits, RandomBitsShare},
    RandomBits,
};
use crate::{
    error::Error,
    ff::PrimeField,
    helpers::TotalRecords,
    protocol::{context::Context, BasicProtocols, RecordId, Substep},
    secret_sharing::Linear as LinearSecretSharing,
};
use std::{
    marker::PhantomData,
    sync::atomic::{AtomicU32, AtomicUsize, Ordering},
};

/// A struct that generates random sharings of bits from the
/// `SolvedBits` protocol. Any protocol who wish to use a random-bits can draw
/// one by calling `generate()`.
///
/// This object is safe to share with multiple threads.  It uses an atomic counter
/// to manage concurrent accesses.
#[derive(Debug)]
pub struct RandomBitsGenerator<F, S, C> {
    /// Happy path touches this generator only. With the right field size, the probability of this
    /// generator failing is very low. For [`Fp32BitPrime`] it fails with $5/2^{32}$ ~ $1^{10^-9}$
    ///
    /// [`Fp31`]: crate::ff::Fp32BitPrime
    default_generator: CountingGenerator<F, S, C>,
    /// If the default generator fails, this one is used until it successfully generates random
    /// sharings of value `v` < [`PRIME`]
    ///
    /// [`PRIME`]: PrimeField::PRIME
    fallback_generator: CountingGenerator<F, S, C>,
    abort_count: AtomicUsize,
}

/// Internal type to generate random bits using [`solved_bits`] protocol using sequential record ids.
#[derive(Debug)]
struct CountingGenerator<F, S, C> {
    counter: AtomicU32,
    ctx: C,
    _marker: PhantomData<(F, S)>,
}

enum RBGStep {
    /// Special communication channel used when values generated using the standard communications
    /// failed to pass the prime test. It is widely inefficient to use, so field must be large enough
    /// so it does not happen often
    FallbackChannel,
}

impl AsRef<str> for RBGStep {
    fn as_ref(&self) -> &str {
        match self {
            RBGStep::FallbackChannel => "fallback",
        }
    }
}

impl Substep for RBGStep {}

impl<F, S, C> RandomBitsGenerator<F, S, C>
where
    F: PrimeField,
    S: LinearSecretSharing<F> + BasicProtocols<C, F>,
    C: Context + RandomBits<F, Share = S>,
{
    #[must_use]
    #[allow(clippy::needless_pass_by_value)] // TODO: pending resolution of TotalRecords::Indeterminate
    pub fn new<I: Into<TotalRecords>>(ctx: C, total_records: I) -> Self {
        drop(total_records); // todo: temporarily, until new infra is in place
                             // todo: remove and use capacity for the default generator
        debug_assert!(ctx.is_total_records_unspecified());
        Self {
            default_generator: CountingGenerator::new(
                ctx.set_total_records(TotalRecords::Indeterminate),
            ),
            fallback_generator: CountingGenerator::new(
                ctx.narrow(&RBGStep::FallbackChannel)
                    .set_total_records(TotalRecords::Indeterminate),
            ),
            abort_count: AtomicUsize::new(0),
        }
    }

    /// Takes the next `RandomBitsShare` that is available.  As the underlying
    /// generator can fail, this will draw from that repeatedly until a value is produced.
    ///
    /// # Errors
    /// This method may fail for number of reasons. Errors include locking the
    /// inner members multiple times, I/O errors while executing MPC protocols,
    /// read from an empty buffer, etc.
    pub async fn generate(&self) -> Result<RandomBitsShare<F, S>, Error> {
        if let Some(v) = self.default_generator.next().await? {
            Ok(v)
        } else {
            loop {
                self.abort_count.fetch_add(1, Ordering::AcqRel);
                if let Some(v) = self.fallback_generator.next().await? {
                    break Ok(v);
                }
            }
        }
    }

    /// Get the number of aborts for this instance.
    #[allow(dead_code)]
    pub fn aborts(&self) -> usize {
        self.abort_count.load(Ordering::Acquire)
    }
}

impl<F, S, C> CountingGenerator<F, S, C>
where
    F: PrimeField,
    S: LinearSecretSharing<F> + BasicProtocols<C, F>,
    C: Context + RandomBits<F, Share = S>,
{
    fn new(ctx: C) -> Self {
        Self {
            counter: AtomicU32::new(0),
            ctx,
            _marker: PhantomData,
        }
    }

    async fn next(&self) -> Result<Option<RandomBitsShare<F, S>>, Error> {
        let i = self.counter.fetch_add(1, Ordering::AcqRel);
        solved_bits(self.ctx.clone(), RecordId::from(i)).await
    }
}

#[cfg(all(test, not(feature = "shuttle")))]
mod tests {
    use std::{iter::zip, sync::atomic::Ordering};

    use futures::future::try_join_all;

    use super::RandomBitsGenerator;
    use crate::{
        ff::Fp31,
        helpers::TotalRecords,
        protocol::{boolean::RandomBitsShare, malicious::MaliciousValidator},
        test_fixture::{join3, Reconstruct, Runner, TestWorld},
    };

    #[tokio::test]
    pub async fn semi_honest() {
        let world = TestWorld::default();
        let [c0, c1, c2] = world.contexts();

        let rbg0 = RandomBitsGenerator::new(c0, 1);
        let rbg1 = RandomBitsGenerator::new(c1, 1);
        let rbg2 = RandomBitsGenerator::new(c2, 1);

        let result = join3(rbg0.generate(), rbg1.generate(), rbg2.generate()).await;
        assert_eq!(rbg0.aborts(), rbg1.aborts());
        assert_eq!(rbg0.aborts(), rbg2.aborts());
        let _: Fp31 = result.reconstruct(); // reconstruct() will validate the value.
    }

    #[tokio::test]
    pub async fn uses_fallback_channel() {
        let world = TestWorld::default();

        world
            .semi_honest((), |ctx, _| async move {
                let rbg = RandomBitsGenerator::new(ctx, TotalRecords::Indeterminate);
                while rbg.abort_count.load(Ordering::Acquire) == 0 {
                    let _: RandomBitsShare<Fp31, _> = rbg.generate().await.unwrap();
                }
            })
            .await;
    }

    #[tokio::test]
    pub async fn malicious() {
        let world = TestWorld::default();
        let contexts = world.contexts();

        let validators = contexts.map(MaliciousValidator::<Fp31>::new);
        let rbg = validators
            .iter()
            .map(|v| RandomBitsGenerator::new(v.context(), 1))
            .collect::<Vec<_>>();

        let m_result = join3(rbg[0].generate(), rbg[1].generate(), rbg[2].generate()).await;
        assert_eq!(rbg[0].aborts(), rbg[1].aborts());
        assert_eq!(rbg[0].aborts(), rbg[2].aborts());

        let result = <[_; 3]>::try_from(
            try_join_all(zip(validators, m_result).map(|(v, m)| v.validate(m)))
                .await
                .unwrap(),
        )
        .unwrap();
        let _: Fp31 = result.reconstruct(); // reconstruct() will validate the value.
    }
}
