// Copyright 2018 The Exonum Team
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//   http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Tests in this module are designed to test details of round timeout handling.

use std::time::Duration;

use crypto::CryptoHash;
use helpers::{Height, Round, ValidatorId};
use messages::{Message, Precommit, Prevote};
use node::state::PROPOSE_REQUEST_TIMEOUT;

use sandbox::{sandbox::timestamping_sandbox, sandbox_tests_helper::*};

/// HANDLE ROUND TIMEOUT:
/// - Ignore if height and round are not the same
/// scenario:
///  - make commit at first round
///  - and verify that at moment when first `round_timeout` is triggered, round remains the same
#[test]
fn handle_round_timeout_ignore_if_height_and_round_are_not_the_same() {
    let sandbox = timestamping_sandbox();

    // option: with transaction
    let tx = gen_timestamping_tx();

    let propose = ProposeBuilder::new(&sandbox)
                .with_tx_hashes(&[tx.hash()]) //ordinary propose, but with this unreceived tx
        .build();

    // this block with transactions should be in real
    let block = BlockBuilder::new(&sandbox)
        .with_tx_hash(&tx.hash())
        .with_state_hash(&sandbox.compute_state_hash(&[tx.raw().clone()]))
        .build();

    let precommit_1 = Precommit::new(
        ValidatorId(1),
        Height(1),
        Round(1),
        &propose.hash(),
        &block.hash(),
        sandbox.time().into(),
        sandbox.s(ValidatorId(1)),
    );
    let precommit_2 = Precommit::new(
        ValidatorId(2),
        Height(1),
        Round(1),
        &propose.hash(),
        &block.hash(),
        sandbox.time().into(),
        sandbox.s(ValidatorId(2)),
    );
    let precommit_3 = Precommit::new(
        ValidatorId(3),
        Height(1),
        Round(1),
        &propose.hash(),
        &block.hash(),
        sandbox.time().into(),
        sandbox.s(ValidatorId(3)),
    );

    sandbox.recv(&precommit_1);
    sandbox.add_time(Duration::from_millis(PROPOSE_REQUEST_TIMEOUT));
    sandbox.send(
        sandbox.a(ValidatorId(1)),
        &make_request_propose_from_precommit(&sandbox, &precommit_1),
    );
    sandbox.send(
        sandbox.a(ValidatorId(1)),
        &make_request_prevote_from_precommit(&sandbox, &precommit_1),
    );

    sandbox.recv(&precommit_2);
    // second addition is required in order to make sandbox time >= propose time because
    // this condition is checked at node/mod.rs->actual_round()
    sandbox.add_time(Duration::from_millis(PROPOSE_REQUEST_TIMEOUT));
    sandbox.send(
        sandbox.a(ValidatorId(2)),
        &make_request_propose_from_precommit(&sandbox, &precommit_2),
    );
    sandbox.send(
        sandbox.a(ValidatorId(2)),
        &make_request_prevote_from_precommit(&sandbox, &precommit_2),
    );
    sandbox.recv(&propose);
    sandbox.recv(&tx);
    sandbox.broadcast(&make_prevote_from_propose(&sandbox, &propose));

    sandbox.assert_state(Height(1), Round(1));
    // Here consensus.rs->handle_majority_precommits()->//Commit is achieved
    sandbox.recv(&precommit_3);
    sandbox.assert_state(Height(2), Round(1));
    sandbox.check_broadcast_status(Height(2), &block.hash());
    sandbox.add_time(Duration::from_millis(0));

    sandbox.add_time(Duration::from_millis(
        sandbox.current_round_timeout() - 2 * PROPOSE_REQUEST_TIMEOUT,
    ));
    // This assert would fail if check for same height is absent in
    // node/consensus.rs->handle_round_timeout()
    sandbox.assert_state(Height(2), Round(1));
}

/// HANDLE ROUND TIMEOUT:
// - add new round timeout
#[test]
fn handle_round_timeout_increment_round_add_new_round_timeout() {
    let sandbox = timestamping_sandbox();

    sandbox.add_time(Duration::from_millis(sandbox.current_round_timeout() - 1));
    sandbox.assert_state(Height(1), Round(1));
    sandbox.add_time(Duration::from_millis(1));
    sandbox.assert_state(Height(1), Round(2));

    // next round timeout is added
    sandbox.add_time(Duration::from_millis(sandbox.current_round_timeout() - 1));
    sandbox.assert_state(Height(1), Round(2));
    sandbox.add_time(Duration::from_millis(1));
    sandbox.assert_state(Height(1), Round(3));
    sandbox.add_time(Duration::from_millis(0));
}

/// idea of the scenario is to become leader
/// then:
///  - propose timeout is added
///   - when propose timeout is triggered - propose is send
#[test]
fn test_send_propose_and_prevote_when_we_are_leader() {
    let sandbox = timestamping_sandbox();

    // round happens
    sandbox.add_time(Duration::from_millis(sandbox.current_round_timeout()));
    sandbox.add_time(Duration::from_millis(
        sandbox.current_round_timeout() + PROPOSE_TIMEOUT,
    ));

    sandbox.assert_state(Height(1), Round(3));

    // ok, we are leader
    let propose = ProposeBuilder::new(&sandbox).build();

    sandbox.broadcast(&propose);
    sandbox.broadcast(&make_prevote_from_propose(&sandbox, &propose));
    sandbox.add_time(Duration::from_millis(0));
}

/// HANDLE ROUND TIMEOUT:
/// - send prevote if locked to propose
/// idea:
///  - lock to propose
///  - trigger `round_timeout`
///  - observe broadcasted prevote
#[test]
fn handle_round_timeout_send_prevote_if_locked_to_propose() {
    // fn test_get_lock_and_send_precommit() {
    let sandbox = timestamping_sandbox();

    let propose = ProposeBuilder::new(&sandbox).build();

    let block = BlockBuilder::new(&sandbox).build();

    sandbox.recv(&propose);
    sandbox.broadcast(&Prevote::new(
        ValidatorId(0),
        Height(1),
        Round(1),
        &propose.hash(),
        NOT_LOCKED,
        sandbox.s(ValidatorId(0)),
    ));

    sandbox.recv(&Prevote::new(
        ValidatorId(1),
        Height(1),
        Round(1),
        &propose.hash(),
        NOT_LOCKED,
        sandbox.s(ValidatorId(1)),
    ));
    sandbox.assert_lock(NOT_LOCKED, None); //do not lock if <2/3 prevotes

    sandbox.recv(&Prevote::new(
        ValidatorId(2),
        Height(1),
        Round(1),
        &propose.hash(),
        NOT_LOCKED,
        sandbox.s(ValidatorId(2)),
    ));
    sandbox.assert_lock(Round(1), Some(propose.hash())); //only if round > locked round

    sandbox.broadcast(&Precommit::new(
        ValidatorId(0),
        Height(1),
        Round(1),
        &propose.hash(),
        &block.hash(),
        sandbox.time().into(),
        sandbox.s(ValidatorId(0)),
    ));
    sandbox.assert_lock(Round(1), Some(propose.hash()));
    sandbox.add_time(Duration::from_millis(0));

    // trigger round_timeout
    sandbox.add_time(Duration::from_millis(sandbox.current_round_timeout()));
    //    sandbox.broadcast(&make_prevote_from_propose(&sandbox, &propose));
    sandbox.broadcast(&Prevote::new(
        ValidatorId(0),
        Height(1),
        Round(2),
        &propose.hash(),
        Round(1),
        sandbox.s(ValidatorId(0)),
    ));
    sandbox.add_time(Duration::from_millis(0));
}

/// HANDLE ROUND TIMEOUT:
///  - handle queued messages
/// idea:
///  - lock to propose
///  - trigger `round_timeout`
///  - observe broadcasted prevote
#[test]
#[should_panic(expected = "Send unexpected message Request(ProposeRequest")]
fn test_handle_round_timeout_queue_prevote_message_from_next_round() {
    let sandbox = timestamping_sandbox();

    sandbox.recv(&Prevote::new(
        ValidatorId(2),
        Height(1),
        Round(2),
        &empty_hash(),
        NOT_LOCKED,
        sandbox.s(ValidatorId(2)),
    ));

    // trigger round_timeout
    sandbox.add_time(Duration::from_millis(sandbox.current_round_timeout()));
    // trigger request_propose_timeout
    sandbox.add_time(Duration::from_millis(PROPOSE_REQUEST_TIMEOUT));
    // observe requestPropose request
    sandbox.add_time(Duration::from_millis(0));
}

/// Check that each consecutive round is longer than previous by the fixed amount
#[test]
fn test_round_timeout_increase() {
    let sandbox = timestamping_sandbox();
    let sandbox_state = SandboxState::new();

    sandbox.add_time(Duration::from_millis(sandbox.first_round_timeout() - 1));
    sandbox.assert_state(Height(1), Round(1));
    sandbox.add_time(Duration::from_millis(1));
    sandbox.assert_state(Height(1), Round(2));

    sandbox.add_time(Duration::from_millis(
        sandbox.first_round_timeout() + sandbox.round_timeout_increase() - 1,
    ));
    sandbox.assert_state(Height(1), Round(2));
    sandbox.add_time(Duration::from_millis(1));
    sandbox.assert_state(Height(1), Round(3));

    //to make sure that there are no unchecked messages from validator 0 we skip round 3
    add_round_with_transactions(&sandbox, &sandbox_state, &[]);
    sandbox.assert_state(Height(1), Round(4));

    sandbox.add_time(Duration::from_millis(
        sandbox.first_round_timeout() + 3 * sandbox.round_timeout_increase() - 1,
    ));
    sandbox.assert_state(Height(1), Round(4));
    sandbox.add_time(Duration::from_millis(1));
    sandbox.assert_state(Height(1), Round(5));
}
