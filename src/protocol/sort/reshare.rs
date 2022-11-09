use crate::ff::Field;
use crate::{
    error::BoxError,
    helpers::{Direction, Role},
    protocol::{context::ProtocolContext, RecordId},
    secret_sharing::Replicated,
};
use embed_doc_image::embed_doc_image;

/// Reshare(i, \[x\])
// This implements reshare algorithm of "Efficient Secure Three-Party Sorting Protocol with an Honest Majority" at communication cost of 2R.
// Input: Pi-1 and Pi+1 know their secret shares
// Output: At the end of the protocol, all 3 helpers receive their shares of a new, random secret sharing of the secret value
#[derive(Debug)]
pub struct Reshare<F> {
    input: Replicated<F>,
}

impl<F: Field> Reshare<F> {
    pub fn new(input: Replicated<F>) -> Self {
        Self { input }
    }

    #[embed_doc_image("reshare", "images/sort/reshare.png")]
    /// Steps
    /// ![Reshare steps][reshare]
    /// 1. While calculating for a helper, we call pseudo random secret sharing (prss) to get random values which match
    ///    with those generated by other helpers (say `rand_left`, `rand_right`)
    ///    `to_helper.left` knows `rand_left` (named r1) and `to_helper.right` knows `rand_right` (named r0)
    /// 2. `to_helper.left` calculates part1 = (a1 + a2) - r2 = Same as (input.left() + input.right()) - r1 from helper POV
    ///    `to_helper.right` calculates part2 = (a3 - r3) = Same as (input.left() - r0) from helper POV
    /// 3. `to_helper.left` and `to_helper.right` exchange their calculated shares
    /// 4. Everyone sets their shares
    ///    `to_helper.left`  = (part1 + part2, `rand_left`)  = (part1 + part2, r1)
    ///    `to_helper`       = (`rand_left`, `rand_right`)     = (r0, r1)
    ///    `to_helper.right` = (`rand_right`, part1 + part2) = (r0, part1 + part2)
    pub async fn execute(
        self,
        ctx: &ProtocolContext<'_, Replicated<F>, F>,
        record_id: RecordId,
        to_helper: Role,
    ) -> Result<Replicated<F>, BoxError> {
        let channel = ctx.mesh();
        let prss = ctx.prss();
        let (r0, r1) = prss.generate_fields(record_id);

        // `to_helper.left` calculates part1 = (input.0 + input.1) - r1 and sends part1 to `to_helper.right`
        // This is same as (a1 + a2) - r2 in the diagram
        if ctx.role() == to_helper.peer(Direction::Left) {
            let part1 = self.input.left() + self.input.right() - r1;
            channel
                .send(to_helper.peer(Direction::Right), record_id, part1)
                .await?;

            // Sleep until `to_helper.right` sends us their part2 value
            let part2 = channel
                .receive(to_helper.peer(Direction::Right), record_id)
                .await?;

            Ok(Replicated::new(part1 + part2, r1))
        } else if ctx.role() == to_helper.peer(Direction::Right) {
            // `to_helper.right` calculates part2 = (input.left() - r0) and sends it to `to_helper.left`
            // This is same as (a3 - r3) in the diagram
            let part2 = self.input.left() - r0;
            channel
                .send(to_helper.peer(Direction::Left), record_id, part2)
                .await?;

            // Sleep until `to_helper.left` sends us their part1 value
            let part1: F = channel
                .receive(to_helper.peer(Direction::Left), record_id)
                .await?;

            Ok(Replicated::new(r0, part1 + part2))
        } else {
            Ok(Replicated::new(r0, r1))
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::Rng;
    use rand::rngs::mock::StepRng;
    use tokio::try_join;

    use crate::{
        ff::Fp31,
        helpers::Role,
        protocol::{sort::reshare::Reshare, QueryId, RecordId},
        test_fixture::{make_contexts, make_world, share, validate_and_reconstruct, TestWorld},
    };

    #[tokio::test]
    pub async fn reshare() {
        let mut rand = StepRng::new(100, 1);
        let mut rng = rand::thread_rng();
        let mut new_reshares_atleast_once = false;
        let world: TestWorld = make_world(QueryId);
        let context = make_contexts::<Fp31>(&world);

        for _ in 0..10 {
            let secret = rng.gen::<u128>();

            let input = Fp31::from(secret);
            let share = share(input, &mut rand);
            let record_id = RecordId::from(0_u32);

            let reshare0 = Reshare::new(share[0]);
            let reshare1 = Reshare::new(share[1]);
            let reshare2 = Reshare::new(share[2]);

            let h0_future = reshare0.execute(&context[0], record_id, Role::H2);
            let h1_future = reshare1.execute(&context[1], record_id, Role::H2);
            let h2_future = reshare2.execute(&context[2], record_id, Role::H2);

            let f = try_join!(h0_future, h1_future, h2_future).unwrap();
            let output_share = validate_and_reconstruct(f);
            assert_eq!(output_share, input);

            if share[0] != f.0 && share[1] != f.1 && share[2] != f.2 {
                new_reshares_atleast_once = true;
                break;
            }
        }
        assert!(new_reshares_atleast_once);
    }
}
