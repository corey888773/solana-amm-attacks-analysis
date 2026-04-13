# Analiza podatności protokołów AMM na ataki typu MEV w sieci Solana

Praca magisterska - Politechnika Krakowska, Wydział Informatyki i Telekomunikacji, 2026

## Usage

```bash
# Build
cargo build

# Tests
cargo test

# Single scenario (default config)
cargo run --bin mev-sim -- -c configs/default.toml

# Parameter sweep (500 combinations, parallel)
cargo run --bin mev-sim -- -c configs/sweep_liquidity.toml --parallel

# Custom output path
cargo run --bin mev-sim -- -c configs/default.toml -o results/my_test.csv
```

## Structure

```
├── crates/amm-math/    # Pure AMM math (constant product, sandwich optimization)
├── simulator/          # CLI binary — runs simulations, exports CSV
├── configs/            # TOML configs (pool params, victim, sweep)
└── results/            # Generated CSV output (gitignored)
```
