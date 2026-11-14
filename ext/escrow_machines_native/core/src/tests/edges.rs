//! Boundaries: refusals, replays, lost state, u64 limits, frames.
use super::{Ship, receipt};
use crate::*;
use matryona_machines::testing::{refuses, unchanged};

#[test]
fn spends_stop_at_the_local_share_even_when_the_fleet_holds_more() {
    let mut left = Ship::new(0, &[10, 5]).expect("config");
    let mut right = Ship::new(1, &[10, 5]).expect("config");
    assert_eq!((left.supply(), left.replicas(), left.local()), (15, 2, 0));
    assert_eq!(left.spend(11), Err(Refusal::Insufficient { available: 10 }));
    assert_eq!(left.spend(0), Err(Refusal::ZeroAmount));
    assert_eq!(left.spend(10), Ok(0));
    assert_eq!(left.spend(1), Err(Refusal::Insufficient { available: 0 }));
    assert_eq!(
        left.refund(11),
        Err(Refusal::ExceedsSpent { outstanding: 10 })
    );
    assert_eq!(left.refund(0), Err(Refusal::ZeroAmount));
    assert_eq!(left.refund(4), Ok(4));
    assert_eq!((left.spent(0), left.refunded(0)), (Some(10), Some(4)));
    assert_eq!(left.outstanding(0), Some(6));
    assert_eq!(
        right.refund(1),
        Err(Refusal::ExceedsSpent { outstanding: 0 })
    );
    right.merge(&left).expect("merge");
    // Hearing about the other side's rights does not make them spendable here.
    assert_eq!(right.balance(0), Some(4));
    assert_eq!(right.spend(6), Err(Refusal::Insufficient { available: 5 }));
    assert_eq!(left.allotted(1), Some(5));
    assert_eq!(left.allotted(2), None);
    assert_eq!(
        (left.spent(2), left.refunded(2), left.outstanding(2)),
        (None, None, None)
    );
    assert_eq!(
        (left.balance(2), left.granted(0, 2), left.granted(2, 0)),
        (None, None, None)
    );
    assert_eq!(left.last_transfer(0, 2), None);
}

#[test]
fn transfers_replay_by_id_and_refuse_stale_or_conflicting_ids() {
    let mut left = Ship::new(0, &[10, 0, 0]).expect("config");
    let mut right = Ship::new(1, &[10, 0, 0]).expect("config");
    assert_eq!(left.transfer(0, 1, 1), Err(Refusal::SelfTransfer));
    assert_eq!(left.transfer(3, 1, 1), Err(Refusal::UnknownReplica));
    assert_eq!(left.transfer(u64::MAX, 1, 1), Err(Refusal::UnknownReplica));
    assert_eq!(left.transfer(1, 1, 0), Err(Refusal::ZeroAmount));
    assert_eq!(
        left.transfer(1, 5, 11),
        Err(Refusal::Insufficient { available: 10 })
    );
    let first = receipt(0, 1, 5, 4, 4);
    assert_eq!(left.transfer(1, 5, 4), Ok(Transfer::Applied(first)));
    let replay = unchanged(&mut left, |left| left.transfer(1, 5, 4));
    assert_eq!(replay, Ok(Transfer::Replayed(first)));
    assert_eq!(replay.map(|t| t.receipt()), Ok(first));
    assert_eq!(left.available(), 6);
    assert_eq!(
        left.transfer(1, 5, 3),
        Err(Refusal::IdConflict { amount: 4 })
    );
    assert_eq!(left.transfer(1, 4, 4), Err(Refusal::StaleId { latest: 5 }));
    assert_eq!(left.last_transfer(0, 1), Mark::new(5, 4));
    // Ids are scoped to the pair: the same id to another replica is fresh.
    let other = receipt(0, 2, 5, 1, 1);
    assert_eq!(left.transfer(2, 5, 1), Ok(Transfer::Applied(other)));
    let second = receipt(0, 1, 6, 2, 6);
    assert_eq!(left.transfer(1, 6, 2), Ok(Transfer::Applied(second)));
    // A replayed id is answered even when the balance could not pay it again.
    assert_eq!(left.spend(3), Ok(0));
    let replay = unchanged(&mut left, |left| left.transfer(1, 6, 2));
    assert_eq!(replay, Ok(Transfer::Replayed(second)));
    assert_eq!(left.transfer(1, 5, 4), Err(Refusal::StaleId { latest: 6 }));

    // Receipts land out of order and more than once.
    assert_eq!(right.accept(&second), Ok(Accepted::Applied));
    assert_eq!(right.available(), 6);
    for replayed in [first, second] {
        let answer = unchanged(&mut right, |right| right.accept(&replayed));
        assert_eq!(answer, Ok(Accepted::Replayed));
    }
    assert_eq!(right.available(), 6);
    assert_eq!(right.granted(0, 1), Some(6));
    assert_eq!(left.accept(&second), Ok(Accepted::Replayed));
    // A bystander may carry the receipt too; it gains nothing spendable.
    let mut third = Ship::new(2, &[10, 0, 0]).expect("config");
    assert_eq!(third.accept(&first), Ok(Accepted::Applied));
    assert_eq!(third.accept(&second), Ok(Accepted::Applied));
    assert_eq!((third.available(), third.granted(0, 1)), (0, Some(6)));
}

#[test]
fn receipts_that_lie_are_refused_without_effect() {
    let mut left = Ship::new(0, &[10, 0]).expect("config");
    let mut right = Ship::new(1, &[10, 0]).expect("config");
    let first = left.transfer(1, 1, 3).expect("transfer").receipt();
    let second = left.transfer(1, 2, 4).expect("transfer").receipt();
    right.accept(&second).expect("accept");
    // Each lie is a receipt consistent with itself: one that is not cannot be
    // built at all (see `constructors_refuse_values_no_call_produces`).
    let cases = [
        (receipt(4, 1, 2, 4, 7), Refusal::UnknownReplica),
        (receipt(0, 4, 2, 4, 7), Refusal::UnknownReplica),
        (receipt(0, 1, 2, 5, 7), Refusal::IdConflict { amount: 4 }),
        (receipt(0, 1, 2, 4, 9), Refusal::Malformed),
        (receipt(0, 1, 3, 4, 7), Refusal::Malformed),
        (receipt(0, 1, 3, 4, 10), Refusal::Malformed),
        (receipt(0, 1, 1, 4, 7), Refusal::Malformed),
    ];
    for (receipt, refusal) in cases {
        refuses(&mut right, refusal, |right| right.accept(&receipt));
    }
    assert_eq!(right.accept(&first), Ok(Accepted::Replayed));
    // The giver knows what it gave: a later id it never issued is a forgery.
    let forged = receipt(0, 1, 3, 1, 8);
    refuses(&mut left, Refusal::Forged, |left| left.accept(&forged));
    let mut fresh = Ship::new(0, &[10, 0]).expect("config");
    assert_eq!(fresh.accept(&first), Err(Refusal::Forged));
    // A receipt that would push the local balance past u64::MAX.
    let mut rich = Ship::new(1, &[0, u64::MAX]).expect("config");
    let gift = receipt(0, 1, 1, 1, 1);
    assert_eq!(rich.accept(&gift), Err(Refusal::Overflow));
    assert_eq!(rich, Ship::new(1, &[0, u64::MAX]).expect("config"));
}

#[test]
fn a_replica_that_lost_state_is_caught_by_merge_and_must_recover_explicitly() {
    let mut left = Ship::new(0, &[10, 5]).expect("config");
    let mut right = Ship::new(1, &[10, 5]).expect("config");
    let snapshot = left;
    assert_eq!(left.spend(4), Ok(6));
    right.merge(&left).expect("merge");
    // The stale generation of replica 0 comes back and meets a peer.
    let mut stale = snapshot;
    refuses(&mut stale, Refusal::Diverged, |stale| stale.merge(&right));
    assert_eq!(stale.recover(&right), Ok(()));
    assert_eq!(stale, left);

    let receipt = left.transfer(1, 1, 6).expect("transfer").receipt();
    right.accept(&receipt).expect("accept");
    let mut stale = snapshot;
    assert_eq!(stale.merge(&right), Err(Refusal::Diverged));
    // It spent again before asking: the adopted history does not add up.
    assert_eq!(stale.spend(10), Ok(0));
    refuses(&mut stale, Refusal::Overdrawn, |stale| {
        stale.recover(&right)
    });

    // It reused a transfer id for a different amount.
    let mut stale = snapshot;
    let reused = stale.transfer(1, 1, 2).expect("transfer").receipt();
    refuses(&mut stale, Refusal::Diverged, |stale| stale.recover(&right));
    refuses(&mut right, Refusal::Diverged, |right| right.merge(&stale));
    let conflict = Refusal::IdConflict { amount: 6 };
    refuses(&mut right, conflict, |right| right.accept(&reused));

    // A peer that is merely behind is not a divergence.
    let mut behind = Ship::new(1, &[10, 5]).expect("config");
    assert_eq!(left.merge(&behind), Ok(()));
    assert_eq!(behind.merge(&left), Ok(()));
    assert_eq!(behind.available(), 11);
}

#[test]
fn views_of_another_allotment_do_not_merge() {
    let mut ship = Ship::new(0, &[10, 5]).expect("config");
    for other in [&[10, 6][..], &[10, 5, 0], &[15]] {
        let other = Ship::new(0, other).expect("config");
        refuses(&mut ship, Refusal::ConfigMismatch, |ship| {
            ship.merge(&other)
        });
        refuses(&mut ship, Refusal::ConfigMismatch, |ship| {
            ship.recover(&other)
        });
    }
}

#[test]
fn configuration_is_checked_against_capacity_and_u64() {
    assert_eq!(Ship::CAPACITY, 4);
    assert_eq!(Ship::new(0, &[]), Err(ConfigError::NoReplicas));
    assert_eq!(Ship::new(0, &[1; 5]), Err(ConfigError::TooManyReplicas));
    assert_eq!(Ship::new(2, &[1, 1]), Err(ConfigError::UnknownReplica));
    assert_eq!(
        Ship::new(u64::MAX, &[1, 1]),
        Err(ConfigError::UnknownReplica)
    );
    assert_eq!(
        Ship::new(0, &[u64::MAX, 1]),
        Err(ConfigError::SupplyOverflow)
    );
    assert_eq!(Escrow::<0>::new(0, &[]), Err(ConfigError::NoReplicas));
    assert_eq!(Escrow::<0>::new(0, &[1]), Err(ConfigError::TooManyReplicas));
    let full = Ship::new(3, &[1, 2, 3, u64::MAX - 6]).expect("config");
    assert_eq!((full.replicas(), full.supply()), (4, u64::MAX));
    assert_eq!(full.available(), u64::MAX - 6);
}

#[test]
fn counters_refuse_at_u64_limits_instead_of_wrapping() {
    let mut left = Ship::new(0, &[u64::MAX, 0]).expect("config");
    let mut right = Ship::new(1, &[u64::MAX, 0]).expect("config");
    let out = left.transfer(1, 1, u64::MAX).expect("transfer").receipt();
    assert_eq!(right.accept(&out), Ok(Accepted::Applied));
    assert_eq!(right.available(), u64::MAX);
    let back = right.transfer(0, 1, u64::MAX).expect("transfer").receipt();
    assert_eq!(left.accept(&back), Ok(Accepted::Applied));
    assert_eq!(left.available(), u64::MAX);
    // The lifetime grant counter for 0 -> 1 is spent even though rights remain.
    refuses(&mut left, Refusal::Overflow, |left| left.transfer(1, 2, 1));

    assert_eq!(left.spend(u64::MAX), Ok(0));
    assert_eq!(left.refund(u64::MAX), Ok(u64::MAX));
    refuses(&mut left, Refusal::Overflow, |left| left.spend(1));
    assert_eq!(left.transfer(1, u64::MAX, 1), Err(Refusal::Overflow));
    right.merge(&left).expect("merge");
    assert_eq!(right.balance(0), Some(i128::from(u64::MAX)));

    let mut ids = Ship::new(0, &[5, 0]).expect("config");
    assert!(ids.transfer(1, u64::MAX, 1).is_ok());
    assert_eq!(
        ids.transfer(1, 0, 1),
        Err(Refusal::StaleId { latest: u64::MAX })
    );
}

#[test]
fn frames_round_trip_and_reject_what_no_history_could_produce() {
    let mut left = Ship::new(0, &[10, 5, 1]).expect("config");
    left.spend(3).expect("spend");
    left.refund(1).expect("refund");
    left.transfer(2, 9, 2).expect("transfer");
    let mut frame = [0_u8; Ship::ENCODED_LEN + 3];
    assert_eq!(left.encode(&mut frame), Ok(Ship::ENCODED_LEN));
    assert_eq!(Ship::decode(&frame), Ok(left));
    let frame = &frame[..Ship::ENCODED_LEN];
    assert_eq!(left.encode(&mut [0; 10]), Err(FrameError::Truncated));
    assert_eq!(
        Ship::decode(&frame[..frame.len() - 1]),
        Err(FrameError::Truncated)
    );
    assert_eq!(Ship::decode(&[]), Err(FrameError::Truncated));
    assert_eq!(Escrow::<3>::decode(frame), Err(FrameError::Shape));
    assert_eq!(Escrow::<5>::decode(frame), Err(FrameError::Truncated));

    let patched = |at: usize, byte: u8| {
        let mut copy = [0_u8; Ship::ENCODED_LEN];
        copy.copy_from_slice(frame);
        copy[at] = byte;
        Ship::decode(&copy)
    };
    assert_eq!(patched(0, 0x61), Err(FrameError::Tag));
    assert_eq!(patched(3, 0), Err(FrameError::Corrupt)); // no replicas
    assert_eq!(patched(3, 5), Err(FrameError::Corrupt)); // more than capacity
    assert_eq!(patched(5, 3), Err(FrameError::Corrupt)); // local not a replica
    assert_eq!(patched(7, 3), Err(FrameError::Corrupt)); // allotment below what was spent
    assert_eq!(patched(7 + 3 * 8, 1), Err(FrameError::Corrupt)); // allotment for nobody
    assert_eq!(patched(7 + 4 * 8, 0x62), Err(FrameError::Corrupt)); // not a counter
    let last_mark = Ship::ENCODED_LEN - 17;
    assert_eq!(patched(last_mark, 2), Err(FrameError::Corrupt));
    assert_eq!(patched(last_mark, 1), Err(FrameError::Corrupt)); // mark without a grant
    assert_eq!(patched(last_mark + 1, 1), Err(FrameError::Corrupt));
    assert_eq!(FrameError::Corrupt.to_string(), "impossible state");
}

#[test]
fn constructors_refuse_values_no_call_produces() {
    let mark = Mark::new(3, 2).map(|mark| (mark.id(), mark.amount()));
    assert_eq!(mark, Some((3, 2)));
    assert_eq!(Mark::new(3, 0), None);
    let fields =
        Receipt::new(0, 1, 7, 4, 9).map(|r| (r.from(), r.to(), r.id(), r.amount(), r.total()));
    assert_eq!(fields, Some((0, 1, 7, 4, 9)));
    assert_eq!(Receipt::new(1, 1, 7, 4, 9), None);
    assert_eq!(Receipt::new(0, 1, 7, 0, 9), None);
    assert_eq!(Receipt::new(0, 1, 7, 10, 9), None);
}
