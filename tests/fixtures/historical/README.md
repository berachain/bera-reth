# Historical Fixtures

This directory holds chain-spec / genesis fixtures that model **closed historical
fork windows** on Berachain mainnet. They are not representative of the live tip
and should not be used as templates for new networks.

They are kept because consensus rules from those windows must remain
re-executable: any node syncing from genesis (and any reorg back into the
window) re-runs those rules. Fixtures here back the e2e tests that pin that
behavior.

## Index

### `eth-genesis-prague3.json`

Models the **Prague3 emergency window**, active on Berachain mainnet for
timestamps `[1762164459, 1762963200)` — 2025-11-03 to 2025-11-12, when Prague4
ended the restrictions. Used by `tests/e2e/prague3_empty_block_test.rs` to
exercise the empty-block builder path that ran while Prague3 was the live tip.

The Prague3 consensus rules enforced by this fixture are validated in
`src/consensus/mod.rs` (`validate_block_post_execution`); see the doc block
above the Prague3 section there for design rationale, including the
intentional asymmetry between the address-scoped checks
(`InternalBalanceChanged`, deposit parser) and the unscoped ERC20 `Transfer`
check.

Bug-bounty findings targeting this fixture (or the rules it covers) against the
**live tip** are out of scope — the gate is inactive on all production
timestamps post-Prague4. Findings about a behavioral split with bera-geth's
`ValidatePrague3Transaction` inside the historical window remain in scope.
