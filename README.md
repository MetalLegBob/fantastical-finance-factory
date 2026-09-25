# The Fantastical Finance Factory — On-Chain Programs

Source code for the seven Solana programs that run **The Fantastical Finance Factory** on mainnet-beta.

- Website: <https://fantasticalfactory.com>
- Documentation: <https://docs.fantasticalfactory.com>. The documentation currently describes V1 and will be
  updated for V2 once the rollout is finished.

This repository holds **only** the deployed on-chain programs. It is published so that anyone can
read the code and rebuild it to the exact bytes running on-chain (see
[Verify the build yourself](#verify-the-build-yourself)).

## The protocol in brief

Three Token-2022 tokens: **CRIME**, **FRAUD** and **PROFIT**.

- CRIME and FRAUD trade against SOL in protocol-owned constant-product pools. Every buy and sell pays a tax.
- Every ~30 minutes (4,500 slots) a VRF result re-rolls the taxes. With 75% probability the "cheap side"
  flips between CRIME and FRAUD. The cheap side's buy tax is 1–4% and the expensive side's buy tax is
  11–14%, rolled independently for each token.
- Tax revenue is split **71% to PROFIT stakers (paid in SOL) / 24% to the Carnage fund / 5% to the treasury**.
- **Carnage**: each epoch has an ~4.3% (11/256) chance to fire. The Carnage fund then burns or sells its
  previously held tokens and market-buys a VRF-chosen token with its SOL.
- The Conversion Vault converts CRIME or FRAUD to PROFIT (and back) at a fixed **100:1**, with no fee.
- A transfer hook restricts CRIME, FRAUD and PROFIT transfers to whitelisted protocol accounts, so all
  trading goes through the taxed pools.

## Programs

| Program | Program ID | What it does |
|---|---|---|
| **AMM** | [`5JsSAL3kJDUWD4ZveYXYZmgm1eVqueesTZVdAvtZg8cR`](https://solscan.io/account/5JsSAL3kJDUWD4ZveYXYZmgm1eVqueesTZVdAvtZg8cR) | Constant-product swap pools (Uniswap V2-style design) with Token-2022 transfer-hook support. Swaps are only reachable through the Tax program. |
| **Tax** | [`43fZGRtmEsP7ExnJE1dbTbNjaP1ncvVmMPusSeksWGEj`](https://solscan.io/account/43fZGRtmEsP7ExnJE1dbTbNjaP1ncvVmMPusSeksWGEj) | Swap router. Reads the current epoch's tax rates, routes swaps into the AMM, applies a 50% minimum-output floor and splits tax 71/24/5. |
| **Epoch** | [`4Heqc8QEjJCspHR8y96wgZBnBfbe3Qb8N6JBZMQt9iw2`](https://solscan.io/account/4Heqc8QEjJCspHR8y96wgZBnBfbe3Qb8N6JBZMQt9iw2) | Epoch state machine. Requests randomness from [ORAO VRF](https://github.com/orao-network/solana-vrf), derives tax rates, triggers and executes Carnage, and finalizes staking rewards each epoch. Epoch transitions are permissionless. |
| **Staking** | [`12b3t1cNiAUoYLiWFEnFa4w6qYxVAiqCWU7KZuzLPYtH`](https://solscan.io/account/12b3t1cNiAUoYLiWFEnFa4w6qYxVAiqCWU7KZuzLPYtH) | PROFIT staking with SOL rewards, using a cumulative reward-per-token accumulator (Synthetix/Quarry pattern). Claiming is permissionless. |
| **Conversion Vault** | [`5uawA6ehYTu69Ggvm3LSK84qFawPKxbWgfngwj15NRJ`](https://solscan.io/account/5uawA6ehYTu69Ggvm3LSK84qFawPKxbWgfngwj15NRJ) | Fixed 100:1 conversion between CRIME/FRAUD and PROFIT. Makes no CPIs except to Token-2022. |
| **Transfer Hook** | [`CiQPQrmQh6BPhb9k7dFnsEs5gKPgdrvNKFc5xie5xVGd`](https://solscan.io/account/CiQPQrmQh6BPhb9k7dFnsEs5gKPgdrvNKFc5xie5xVGd) | Token-2022 transfer hook that only allows transfers involving whitelisted protocol accounts. Makes no outbound CPIs. |
| **Vote** | [`8xeFnfKe6CrzS2MZoP9qVhRCAR1KX8isDpLNuurP9Lsd`](https://solscan.io/account/8xeFnfKe6CrzS2MZoP9qVhRCAR1KX8isDpLNuurP9Lsd) | Weekly stake-weighted vote by PROFIT stakers that chooses how the following week's protocol surplus is used. It records the result only; it holds no funds. |

### Tokens

| Token | Mint |
|---|---|
| CRIME | [`cRiMEhAxoDhcEuh3Yf7Z2QkXUXUMKbakhcVqmDsqPXc`](https://solscan.io/token/cRiMEhAxoDhcEuh3Yf7Z2QkXUXUMKbakhcVqmDsqPXc) |
| FRAUD | [`FraUdp6YhtVJYPxC2w255yAbpTsPqd8Bfhy9rC56jau5`](https://solscan.io/token/FraUdp6YhtVJYPxC2w255yAbpTsPqd8Bfhy9rC56jau5) |
| PROFIT | [`pRoFiTj36haRD5sG2Neqib9KoSrtdYMGrM7SEkZetfR`](https://solscan.io/token/pRoFiTj36haRD5sG2Neqib9KoSrtdYMGrM7SEkZetfR) |

All three have **no mint authority and no freeze authority**, so no new tokens can ever be minted.

### How the programs call each other

```
                 ┌────────────── Epoch ──────────────┐
                 │  (ORAO VRF, Carnage, rewards)     │
                 ▼                                   ▼
User ──► Tax ──► AMM ──► Token-2022 ──► Transfer Hook   Staking ◄── Tax (reward deposits)
          │
          └──► (tax split: Staking / Carnage fund / treasury)

User ──► Conversion Vault ──► Token-2022 ──► Transfer Hook
User ──► Vote (reads Staking accounts; makes no CPIs)
```

## Governance and authorities

- Every program's **upgrade authority** is the Squads v4 multisig vault
  [`GDY4Qu3xGNGZxXdLs1h6eoMXZgJ9aPpv7jtCaqzMoDcN`](https://solscan.io/account/GDY4Qu3xGNGZxXdLs1h6eoMXZgJ9aPpv7jtCaqzMoDcN)
  (multisig [`Db2Q58tTRrYDVFnybbcxNqb1DG4oLXkAKV3cL1QNMLh1`](https://solscan.io/account/Db2Q58tTRrYDVFnybbcxNqb1DG4oLXkAKV3cL1QNMLh1)).
  It is a **2-of-3 multisig and all three members are hardware wallets**.
- **Timelock:** the multisig's timelock is currently **0** while the v2 rollout is in progress. It will be
  restored to **4 hours (14,400 seconds)** when the rollout is complete.
- The same vault is the transfer-hook authority on all three mints and the authority of the protocol's
  admin configuration accounts.
- **No authority has been burned.** Keeping upgradeability lets the team patch critical bugs.
- **Liquidity:** the pools issue no LP tokens and their liquidity is protocol-owned. The AMM includes a
  governed `remove_liquidity_double_sided` instruction. Only the multisig can sign it, and it can only
  send an approved share of both reserves to allowlisted protocol wallets.

## Repository layout

```
programs/
  amm/                Constant-product AMM
  tax-program/        Swap router + tax split
  epoch-program/      Epoch state machine, VRF, Carnage
  staking/            PROFIT staking, SOL rewards
  conversion-vault/   100:1 CRIME/FRAUD <-> PROFIT
  transfer-hook/      Token-2022 whitelist hook
  vote-program/       Weekly surplus-disposal vote
crates/arb-test-support/   Test helper (dev-dependency only, not deployed)
idl/                  Anchor IDLs generated from this source
verification/         Expected on-chain executable hashes
```

Each program is compiled for the cluster it is deployed to. The default build (no features) is the
**mainnet** build. `--features devnet` selects devnet addresses.

> **Tests are not included.** The development test suites depend on internal tooling and fixtures that
> are not part of this repository. Some in-source `#[cfg(test)]` modules reference those files, so
> `cargo test` will not compile here. This has no effect on the deployed program build.

## Verify the build yourself

The deployed binaries are reproducible with
[`solana-verify`](https://github.com/Ellipsis-Labs/solana-verifiable-build), using the official
`solanafoundation/solana-verifiable-build` Docker images pinned by digest.

Requirements: Docker, Rust, and `solana-verify` 0.4.11
(`cargo install solana-verify --version 0.4.11 --locked`).

Two build images are used, because the programs were last deployed at different times:

| Image | Digest | Programs |
|---|---|---|
| Solana 3.0.13 | `sha256:2ee1f4c3f0db0e1c1107ffb978b1d16b792194148852f31891adf3ee85970259` | AMM, Transfer Hook, Tax, Staking, Conversion Vault, Vote |
| Solana 2.3.0 | `sha256:1e6f3097794495fa968c9e07d4da6adcdabf28efc6ddb0314c2e9e72b57a7a34` | Epoch |

```bash
# Example: AMM (Solana 3.0.13 image)
IMAGE=solanafoundation/solana-verifiable-build@sha256:2ee1f4c3f0db0e1c1107ffb978b1d16b792194148852f31891adf3ee85970259

# 1. Build in the pinned Docker image
solana-verify build --base-image "$IMAGE" --library-name amm

# 2. Hash your build
solana-verify get-executable-hash target/deploy/amm.so

# 3. Hash what is deployed on mainnet
solana-verify get-program-hash -u mainnet-beta 5JsSAL3kJDUWD4ZveYXYZmgm1eVqueesTZVdAvtZg8cR
```

Steps 2 and 3 must print the same hash. For Epoch, use the Solana 2.3.0 image and
`--library-name epoch_program`.

> When switching between the two images, delete `target/` first (`rm -rf target`). Build
> artifacts from one toolchain will make the other image's build fail.

The library name, build image and expected hash for every program are in
[`verification/mainnet-hashes.json`](verification/mainnet-hashes.json):

| Program | Library | Image | Expected executable hash |
|---|---|---|---|
| AMM | `amm` | 3.0.13 | `57a7502842a1121860d868fab2738a32ddd8132b9ea86ed3cefa761d1984641a` |
| Transfer Hook | `transfer_hook` | 3.0.13 | `f8ebebe5d2ee7e9b624eab9775a7d5291e4d1b93c55ef89e4212ae1682218e62` |
| Tax | `tax_program` | 3.0.13 | `c12235eb308c435f75e387d2b153db37343c492ffaf62f77dcef1445a885ac29` |
| Epoch | `epoch_program` | 2.3.0 | `ac5522b87927cb3610e2e647bbd35afb75a8a0df9015644a54e478562f81a363` |
| Staking | `staking` | 3.0.13 | `66fea0a119f89e306b4d50667db3ee34bec5b79ce6441027852e6254240c2eb7` |
| Conversion Vault | `conversion_vault` | 3.0.13 | `c4c92ba2af701fde91ed377e9e39d689760ac115bcecb3a3a534cc9a4764a0fd` |
| Vote | `vote_program` | 3.0.13 | `8eeb3933cc5e1168abd78c77d2bcff10fb58fc4ba82ed49733f903213bd567a7` |

The [Reproduce mainnet hashes](.github/workflows/reproduce-hashes.yml) GitHub Action runs the same
check for all seven programs.

On-chain verification through the OtterSec verification API (the "verified" badge on Solscan) is
planned.

## Security

Please report vulnerabilities privately. See [SECURITY.md](SECURITY.md).

## License

Copyright (C) 2026 The Fantastical Finance Factory contributors.

This program is free software: you can redistribute it and/or modify it under the terms of the GNU
General Public License as published by the Free Software Foundation, either **version 3 of the
License, or (at your option) any later version** (`GPL-3.0-or-later`). See [LICENSE](LICENSE).

This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even
the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.

Third-party dependencies keep their own licenses.
