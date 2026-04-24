# Analiza podatności protokołów AMM na ataki typu MEV w sieci Solana

Praca magisterska - Politechnika Krakowska, Wydział Informatyki i Telekomunikacji, 2026

---

## What this project does

A Rust-based simulator for studying MEV sandwich attacks on Solana Automated
Market Makers. Pure off-chain math (`amm-math`) drives parameterized scenario
sweeps, and the same math crate backs an on-chain Anchor program so that
simulation results can be cross-validated against real program execution
inside LiteSVM (planned).

## Architecture

```
      ┌────────────────────────┐
      │   configs/*.toml       │   pool, victim, costs, sweep ranges
      └───────────┬────────────┘
                  │ figment (TOML loader)
                  ▼
      ┌────────────────────────┐
      │   simulator (mev-sim)  │   clap CLI + rayon parallel sweep
      │   ─ engine             │
      │   ─ scenarios          │
      │   ─ output (csv)       │
      └─────┬──────────────┬───┘
            │ uses         │ loads amm.so (planned)
            ▼              ▼
  ┌──────────────────┐   ┌────────────────────────┐
  │  amm-math        │   │  LiteSVM   (planned)   │
  │  ─ constant      │◄──┤  in-process Solana VM  │
  │    product swap  │   └───────────┬────────────┘
  │  ─ sandwich      │               │ invokes
  │    optimizer     │               ▼
  │  (pure, no_std-  │   ┌────────────────────────┐
  │   friendly)      │──►│  programs/amm          │
  └──────────────────┘   │  Anchor on-chain prog  │
       shared crate      │  (cdylib: amm.so)      │
                         └────────────────────────┘

      ┌────────────────────────┐
      │   results/*.csv        │   per-scenario metrics
      └───────────┬────────────┘
                  │
                  ▼
      ┌────────────────────────┐
      │   Jupyter notebook     │   plots, statistics (planned)
      └────────────────────────┘
```

The `amm-math` crate is the single source of truth for swap math. The
off-chain simulator and the on-chain Anchor program link the same
functions, so on-chain validation reduces to comparing pool state after
executing the same trade sequence in both environments.

## Sandwich attack data flow

A sandwich brackets a victim swap with an attacker frontrun (same
direction) and backrun (opposite direction). Reserves evolve across
three steps:

```
  step 0: initial pool
  ┌──────────────────────┐
  │ reserve_A = R_a      │   price = R_b / R_a
  │ reserve_B = R_b      │
  └──────────────────────┘
            │
            │  (1) FRONTRUN   attacker swaps dA_f of A -> B
            ▼                 price of B rises
  ┌──────────────────────┐
  │ reserve_A = R_a + dA │   victim now faces worse price
  │ reserve_B = R_b - dB │
  └──────────────────────┘
            │
            │  (2) VICTIM     swaps dA_v of A -> B at degraded price
            ▼                 (may exceed slippage_tolerance_bps)
  ┌──────────────────────┐
  │ reserve_A +=  dA_v   │
  │ reserve_B -=  dB_v'  │
  └──────────────────────┘
            │
            │  (3) BACKRUN    attacker swaps their dB back to A
            ▼                 realizing profit vs. step 0
  ┌──────────────────────┐
  │ reserve_A = R_a'     │
  │ reserve_B = R_b'     │   attacker_pnl = A_out - dA_f - fees - tip
  └──────────────────────┘
```

Each scenario records: attacker profit/loss, victim slippage, pool price
drift, gas and Jito tip costs, and whether the victim's slippage
tolerance aborted the trade.

## Directory structure

```
magisterka/
├── Cargo.toml              # workspace manifest (3 members)
├── crates/
│   └── amm-math/           # pure math: constant product + sandwich optimizer
│       └── src/{lib,constant_product,sandwich,types}.rs
├── programs/
│   └── amm/                # on-chain Anchor program (cdylib -> amm.so)
│       └── src/{lib,constants,errors}.rs
│       └── src/instructions/, src/state/
├── simulator/              # CLI binary `mev-sim`
│   └── src/{main,config,engine,scenarios,output}.rs
│   └── src/strategies/
├── configs/                # TOML inputs
│   ├── default.toml
│   └── sweep_liquidity.toml
└── results/                # generated CSV (gitignored)
```

## Usage

```bash
# Build the workspace (host targets)
cargo build

# Run tests (includes amm-math proptest invariants)
cargo test

# Single scenario from default config
cargo run --bin mev-sim -- -c configs/default.toml

# Parameter sweep, parallelized with rayon
cargo run --bin mev-sim -- -c configs/sweep_liquidity.toml --parallel

# Custom output path
cargo run --bin mev-sim -- -c configs/default.toml -o results/my_test.csv

# Build the on-chain program to SBF bytecode (produces target/deploy/amm.so)
cargo-build-sbf --manifest-path programs/amm/Cargo.toml
```

The resulting `amm.so` will be loaded into LiteSVM by the simulator for
on-chain validation once that integration lands (TODO; currently blocked
on a `solana-keypair` / `five8` upstream bug, see `programs/amm/Cargo.toml`).

## Tech stack

- Rust 2021, Cargo workspace (3 crates)
- Anchor 1.0.1 (`anchor-lang`, `anchor-spl`) for the on-chain program
- Solana SDK 3.x (transitive, via Anchor)
- LiteSVM — in-process Solana VM for on-chain validation (planned)
- CLI stack: `clap` (args), `figment` (TOML config), `rayon` (parallel
  sweeps), `csv` + `serde` (output), `itertools` (combinatorics)
- `proptest` for math-invariant testing in `amm-math`
