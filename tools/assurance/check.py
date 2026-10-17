#!/usr/bin/env python3
"""Fail-closed local/CI assurance checks. Python 3.11+, standard library only."""
from __future__ import annotations
import argparse
import datetime as dt
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
import tomllib

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from source_pins import matches_source_pin

DOMAIN = ('convex', 'convex-core', 'convex-math', 'convex-curves', 'convex-bonds', 'convex-rates', 'convex-credit', 'convex-analytics', 'convex-portfolio')
FORBIDDEN = {'tokio', 'async-std', 'smol', 'reqwest', 'hyper', 'axum', 'tonic', 'sqlx', 'redb', 'rusqlite', 'postgres', 'tokio-postgres', 'rdkafka', 'lapin', 'async-nats', 'blpapi', 'convex-engine', 'convex-server', 'convex-ports'}
TREASURY_RUNS = (
    'python3 tests/scripts/treasury_bill_reference.py check',
    'cargo test --locked -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test treasury_bill',
    'cargo run --locked -p convex --example treasury_bill',
    'cargo run --locked -p convex --example treasury_bill --no-default-features',
    'cargo run --locked -p convex --example treasury_bill --features schemars',
    'python3 tests/scripts/treasury_bill_check.py --binary target/debug/examples/treasury_bill',
    'python3 tests/scripts/treasury_bill_schema.py --binary target/debug/examples/treasury_bill',
    'cargo build --locked -p convex-ffi',
    'python3 tests/scripts/treasury_bill_parity.py prepare --binary target/debug/examples/treasury_bill --output target/treasury-bill-parity.json',
    'python3 tests/scripts/treasury_bill_parity.py compare --library target/debug/libconvex_ffi.so --parity target/treasury-bill-parity.json',
)

REQUIRED_RUNS = {
    'assurance': TREASURY_RUNS + (
        'cargo test --locked -p convex-core --test quote_marks',
        'cargo test --locked -p convex-core --test quote_marks --no-default-features',
        'cargo test --locked -p convex-core --test quote_marks --features schemars',
        'cargo test --locked -p convex-curves --test native_discount',
        'cargo test --locked -p convex-curves --test native_discount --no-default-features',
        'cargo test --locked -p convex-curves --test native_discount --features schemars',
        'cargo test --locked -p convex-analytics --test quote_wire',
        'cargo test --locked -p convex-analytics --test quote_wire --no-default-features',
        'cargo test --locked -p convex-analytics --test quote_wire --features schemars',
        'cargo test --locked -p convex-ffi --test quote_rpc',
        'cargo test --locked -p convex-core --test numeric_decimal',
        'cargo test --locked -p convex-core --test numeric_decimal --no-default-features',
        'cargo test --locked -p convex-core --test numeric_decimal --features schemars',
        'cargo test --locked -p convex-ffi --test constructor_inputs',
        'cargo test --locked -p convex-server --test retired_liquidity_route',
        'cargo test --locked -p convex-analytics --test coupon_parameter_hedges',
        'cargo test --locked -p convex --test coupon_parameter_hedges',
        'cargo run --locked -p convex --example coupon_parameter_hedges',
        'cargo test --locked -p convex-analytics --test coupon_parameter_hedges --no-default-features',
        'cargo test --locked -p convex --test coupon_parameter_hedges --no-default-features',
        'cargo run --locked -p convex --example coupon_parameter_hedges --no-default-features',
        'cargo test --locked -p convex-analytics --test coupon_parameter_hedges --features schemars',
        'cargo test --locked -p convex --test coupon_parameter_hedges --features schemars',
        'cargo run --locked -p convex --example coupon_parameter_hedges --features schemars',
        'cargo test --locked -p convex-ffi -p convex-mcp -p convex-wasm --test coupon_parameter_hedges',
        'cargo test --locked -p convex-analytics --test coupon_parameter_inverse',
        'cargo test --locked -p convex --test coupon_parameter_inverse',
        'cargo run --locked -p convex --example coupon_parameter_inverse',
        'cargo test --locked -p convex-analytics --test coupon_parameter_inverse --no-default-features',
        'cargo test --locked -p convex --test coupon_parameter_inverse --no-default-features',
        'cargo run --locked -p convex --example coupon_parameter_inverse --no-default-features',
        'cargo test --locked -p convex-analytics --test coupon_parameter_inverse --features schemars',
        'cargo test --locked -p convex --test coupon_parameter_inverse --features schemars',
        'cargo run --locked -p convex --example coupon_parameter_inverse --features schemars',
        'cargo test --locked -p convex-ffi -p convex-mcp -p convex-wasm --test coupon_parameter_inverse',
        'cargo test --locked -p convex-analytics --test coupon_parameter_directional',
        'cargo test --locked -p convex --test coupon_parameter_directional',
        'cargo run --locked -p convex --example coupon_parameter_directional',
        'cargo test --locked -p convex-analytics --test coupon_parameter_directional --no-default-features',
        'cargo test --locked -p convex --test coupon_parameter_directional --no-default-features',
        'cargo run --locked -p convex --example coupon_parameter_directional --no-default-features',
        'cargo test --locked -p convex-analytics --test coupon_parameter_directional --features schemars',
        'cargo test --locked -p convex --test coupon_parameter_directional --features schemars',
        'cargo run --locked -p convex --example coupon_parameter_directional --features schemars',
        'cargo test --locked -p convex-ffi -p convex-mcp -p convex-wasm --test coupon_parameter_directional',
        'cargo test --locked -p convex-analytics --test coupon_directional_risk',
        'cargo test --locked -p convex --test coupon_directional_risk',
        'cargo run --locked -p convex --example coupon_directional_risk',
        'cargo test --locked -p convex-analytics --test coupon_directional_risk --no-default-features',
        'cargo test --locked -p convex --test coupon_directional_risk --no-default-features',
        'cargo run --locked -p convex --example coupon_directional_risk --no-default-features',
        'cargo test --locked -p convex-analytics --test coupon_directional_risk --features schemars',
        'cargo test --locked -p convex --test coupon_directional_risk --features schemars',
        'cargo run --locked -p convex --example coupon_directional_risk --features schemars',
        'cargo test --locked -p convex-ffi -p convex-mcp -p convex-wasm --test coupon_directional_risk',
        'cargo test --locked -p convex-analytics --test coupon_parameter_derivatives',
        'cargo test --locked -p convex --test coupon_parameter_derivatives',
        'cargo run --locked -p convex --example coupon_parameter_derivatives',
        'cargo test --locked -p convex-analytics --test coupon_parameter_derivatives --no-default-features',
        'cargo test --locked -p convex --test coupon_parameter_derivatives --no-default-features',
        'cargo run --locked -p convex --example coupon_parameter_derivatives --no-default-features',
        'cargo test --locked -p convex-analytics --test coupon_parameter_derivatives --features schemars',
        'cargo test --locked -p convex --test coupon_parameter_derivatives --features schemars',
        'cargo run --locked -p convex --example coupon_parameter_derivatives --features schemars',
        'cargo test --locked -p convex-ffi -p convex-mcp -p convex-wasm --test coupon_parameter_derivatives',
        'cargo test --locked -p convex-curves --test curve_state',
        'cargo test --locked -p convex-analytics --test coupon_curve_state',
        'cargo test --locked -p convex --test coupon_curve_state',
        'cargo run --locked -p convex --example coupon_curve_state',
        'cargo test --locked -p convex-curves --test curve_state --no-default-features',
        'cargo test --locked -p convex-analytics --test coupon_curve_state --no-default-features',
        'cargo test --locked -p convex --test coupon_curve_state --no-default-features',
        'cargo run --locked -p convex --example coupon_curve_state --no-default-features',
        'cargo test --locked -p convex-curves --test curve_state --features schemars',
        'cargo test --locked -p convex-analytics --test coupon_curve_state --features schemars',
        'cargo test --locked -p convex --test coupon_curve_state --features schemars',
        'cargo run --locked -p convex --example coupon_curve_state --features schemars',
        'cargo test --locked -p convex-ffi -p convex-mcp -p convex-wasm --test coupon_curve_state',
        'cargo test --locked -p convex-analytics --test coupon_model_scenarios',
        'cargo test --locked -p convex --test coupon_model_scenarios',
        'cargo run --locked -p convex --example coupon_model_scenarios',
        'cargo test --locked -p convex-analytics --test coupon_model_scenarios --no-default-features',
        'cargo test --locked -p convex --test coupon_model_scenarios --no-default-features',
        'cargo run --locked -p convex --example coupon_model_scenarios --no-default-features',
        'cargo test --locked -p convex-analytics --test coupon_model_scenarios --features schemars',
        'cargo test --locked -p convex --test coupon_model_scenarios --features schemars',
        'cargo run --locked -p convex --example coupon_model_scenarios --features schemars',
        'cargo test --locked -p convex-ffi -p convex-mcp -p convex-wasm --test coupon_model_scenarios',
        'cargo test --locked -p convex-analytics --test coupon_graph_risk',
        'cargo test --locked -p convex --test coupon_graph_risk',
        'cargo run --locked -p convex --example coupon_graph_risk',
        'cargo test --locked -p convex-analytics --test coupon_graph_risk --no-default-features',
        'cargo test --locked -p convex --test coupon_graph_risk --no-default-features',
        'cargo run --locked -p convex --example coupon_graph_risk --no-default-features',
        'cargo test --locked -p convex-analytics --test coupon_graph_risk --features schemars',
        'cargo test --locked -p convex --test coupon_graph_risk --features schemars',
        'cargo run --locked -p convex --example coupon_graph_risk --features schemars',
        'cargo test --locked -p convex-ffi -p convex-mcp -p convex-wasm --test coupon_graph_risk',
        'cargo test --locked -p convex-analytics --test whole_coupon_normal',
        'cargo test --locked -p convex --test whole_coupon_normal',
        'cargo run --locked -p convex --example whole_coupon_normal',
        'cargo test --locked -p convex-analytics --test whole_coupon_normal --no-default-features',
        'cargo test --locked -p convex --test whole_coupon_normal --no-default-features',
        'cargo run --locked -p convex --example whole_coupon_normal --no-default-features',
        'cargo test --locked -p convex-analytics --test whole_coupon_normal --features schemars',
        'cargo test --locked -p convex --test whole_coupon_normal --features schemars',
        'cargo run --locked -p convex --example whole_coupon_normal --features schemars',
        'cargo test --locked -p convex-ffi -p convex-mcp -p convex-wasm --test whole_coupon_normal',
        'cargo test --locked -p convex-bonds --test floating_coupon_options',
        'cargo test --locked -p convex-analytics --test floating_coupon_options',
        'cargo test --locked -p convex --test floating_coupon_options',
        'cargo run --locked -p convex --example floating_coupon_options',
        'cargo test --locked -p convex-bonds --test floating_coupon_options --no-default-features',
        'cargo test --locked -p convex-analytics --test floating_coupon_options --no-default-features',
        'cargo test --locked -p convex --test floating_coupon_options --no-default-features',
        'cargo run --locked -p convex --example floating_coupon_options --no-default-features',
        'cargo test --locked -p convex-bonds --test floating_coupon_options --features schemars',
        'cargo test --locked -p convex-analytics --test floating_coupon_options --features schemars',
        'cargo test --locked -p convex --test floating_coupon_options --features schemars',
        'cargo run --locked -p convex --example floating_coupon_options --features schemars',
        'cargo test --locked -p convex-ffi -p convex-mcp -p convex-wasm --test floating_coupon_options',
        'cargo test --locked -p convex-wasm --test optional_retirement',
        'cargo test --locked -p convex-ffi --test yas_output_retirement',
        'cargo test --locked -p convex-wasm --lib ingress::tests',
        'cargo test --locked -p convex-wasm --lib checked_inputs_tests',
        'cargo test --locked -p convex-wasm --lib cash_components_tests',
        'cargo test --locked -p convex-bonds --test dated_cash_risk',
        'cargo test --locked -p convex-wasm --lib dated_yield_tests',
        'cargo test --locked -p convex-core --test cashflow_primitives',
        'cargo test --locked -p convex --test native_primitives',
        'cargo run --locked -p convex --example native_primitives',
        'cargo test --locked -p convex-core --test cashflow_primitives --no-default-features',
        'cargo test --locked -p convex --test native_primitives --no-default-features',
        'cargo run --locked -p convex --example native_primitives --no-default-features',
        'cargo test --locked -p convex-core --test cashflow_primitives --features schemars',
        'cargo test --locked -p convex --test native_primitives --features schemars',
        'cargo run --locked -p convex --example native_primitives --features schemars',
        'python3 tests/scripts/native_primitives_reference.py check',
        'cargo test --locked -p convex-curves --test curve_contracts',
        'cargo test --locked -p convex-curves --test curve_numerics',
        'cargo test --locked -p convex-curves --test curve_families',
        'cargo test --locked -p convex-curves --test curve_methods',
        'cargo test --locked -p convex-curves --test monotone_contracts',
        'cargo test --locked -p convex-curves --test ufr_contracts',
        'cargo test --locked -p convex --test native_curves',
        'cargo test --locked -p convex-curves --test curve_contracts --no-default-features',
        'cargo test --locked -p convex-curves --test curve_numerics --no-default-features',
        'cargo test --locked -p convex-curves --test curve_families --no-default-features',
        'cargo test --locked -p convex-curves --test curve_methods --no-default-features',
        'cargo test --locked -p convex-curves --test monotone_contracts --no-default-features',
        'cargo test --locked -p convex-curves --test ufr_contracts --no-default-features',
        'cargo test --locked -p convex --test native_curves --no-default-features',
        'cargo test --locked -p convex-curves --test curve_contracts --features schemars',
        'cargo test --locked -p convex-curves --test curve_numerics --features schemars',
        'cargo test --locked -p convex-curves --test curve_families --features schemars',
        'cargo test --locked -p convex-curves --test curve_methods --features schemars',
        'cargo test --locked -p convex-curves --test monotone_contracts --features schemars',
        'cargo test --locked -p convex-curves --test ufr_contracts --features schemars',
        'cargo test --locked -p convex --test native_curves --features schemars',
        'python3 tests/scripts/term_decimals_reference.py check',
        'python3 tests/scripts/swap_term_decimals_reference.py check',
        'cargo test --locked -p convex --test term_decimals',
        'cargo run --locked -p convex --example term_decimals',
        'cargo run --locked -p convex --example term_decimals --no-default-features',
        'cargo run --locked -p convex --example term_decimals --features schemars',
        'python3 tests/scripts/portable_math_reference.py check',
        'cargo test --locked -p convex --test portable_math',
        'cargo run --locked -p convex --example portable_math',
        'cargo run --locked -p convex --example portable_math --no-default-features',
        'cargo run --locked -p convex --example portable_math --features schemars',
        'python3 tests/scripts/zero_terms_reference.py check',
        'python3 tests/scripts/zero_terms_boundary_reference.py check',
        'cargo test --locked -p convex --test zero_terms',
        'cargo run --locked -p convex --example zero_terms',
        'cargo run --locked -p convex --example zero_terms --no-default-features',
        'cargo run --locked -p convex --example zero_terms --features schemars',
        'python3 tests/scripts/zero_methods_reference.py check',
        'cargo test --locked -p convex --test zero_methods',
        'cargo run --locked -p convex --example zero_methods',
        'cargo run --locked -p convex --example zero_methods --no-default-features',
        'cargo run --locked -p convex --example zero_methods --features schemars',
        'python3 tests/scripts/bill_methods_reference.py check',
        'python3 tests/scripts/bill_methods_boundary_reference.py check',
        'cargo test --locked -p convex --test bill_methods',
        'cargo run --locked -p convex --example bill_methods',
        'cargo run --locked -p convex --example bill_methods --no-default-features',
        'cargo run --locked -p convex --example bill_methods --features schemars',
        'python3 tests/scripts/simple_bill_reference.py check',
        'cargo test --locked -p convex-analytics --test simple_bill_yield',
        'python3 tools/assurance/check.py',
        'python3 -m unittest discover -s tools/assurance -p "test_*.py"',
        'python3 -m unittest discover -s tools/library -p "test_*.py"',
        'cargo run --locked -p convex --example inventory_probe',
        'cargo run --locked -p convex --example inventory_probe --no-default-features',
        'cargo run --locked -p convex --example inventory_probe --features schemars',
        'cargo test --locked -p convex-math -p convex-core -p convex-ffi',
        'cargo run --locked -p convex --example cashflow_primitives',
        'cargo run --locked -p convex --example cashflow_primitives --no-default-features',
        'cargo run --locked -p convex --example cashflow_primitives --features schemars',
        'cargo run --locked -p convex --example curve_contracts',
        'cargo run --locked -p convex --example curve_contracts --no-default-features',
        'cargo run --locked -p convex --example curve_contracts --features schemars',
        'cargo run --locked -p convex --example curve_numerics',
        'cargo run --locked -p convex --example curve_numerics --no-default-features',
        'cargo run --locked -p convex --example curve_numerics --features schemars',
        'cargo run --locked -p convex --example bond_quotes',
        'cargo run --locked -p convex --example bond_quotes --no-default-features',
        'cargo run --locked -p convex --example bond_quotes --features schemars',
        'cargo run --locked -p convex --example bond_contracts',
        'cargo run --locked -p convex --example bond_contracts --no-default-features',
        'cargo run --locked -p convex --example bond_contracts --features schemars',
        'cargo run --locked -p convex --example optional_bonds',
        'cargo run --locked -p convex --example optional_bonds --no-default-features',
        'cargo run --locked -p convex --example optional_bonds --features schemars',
        'cargo test --locked -p convex-rates',
        'cargo clippy --locked -p convex-rates --all-targets -- -D warnings',
        'cargo run --locked -p convex --example rates_contracts',
        'cargo run --locked -p convex --example rates_contracts --no-default-features',
        'cargo run --locked -p convex --example rates_contracts --features schemars',
        'python3 tests/scripts/swap_valuation_reference.py check',
        'cargo test --locked -p convex-analytics --test swap_calibration',
        'cargo test --locked -p convex-core --test overnight_prefix -p convex-curves --test product_composition -p convex-engine --test swap_curves',
        'cargo run --locked -p convex --example swap_valuation',
        'cargo run --locked -p convex --example swap_valuation --no-default-features',
        'cargo run --locked -p convex --example swap_valuation --features schemars',
        'cargo test --locked -p convex-credit',
        'cargo clippy --locked -p convex-credit --all-targets -- -D warnings',
        'python3 tests/scripts/isda_cds_compare.py check',
        'cargo run --locked -p convex --example cds_valuation',
        'cargo run --locked -p convex --example cds_valuation --no-default-features',
        'cargo run --locked -p convex --example cds_valuation --features schemars',
        'cargo test --locked -p convex-analytics --test credit_calibration',
        'python3 tests/scripts/isda_credit_compare.py check',
        'cargo run --locked -p convex --example credit_calibration',
        'cargo run --locked -p convex --example credit_calibration --no-default-features',
        'cargo run --locked -p convex --example credit_calibration --features schemars',
        'cargo test --locked -p convex-analytics -p convex-ports -p convex-ext-file -p convex-ext-redb --test futures_reference',
        'cargo clippy --locked -p convex-analytics -p convex-ports -p convex-ext-file -p convex-ext-redb --test futures_reference --all-features -- -D warnings',
        'cargo run --locked -p convex --example futures_reference',
        'cargo run --locked -p convex --example futures_reference --no-default-features',
        'cargo run --locked -p convex --example futures_reference --features schemars',
        'cargo test --locked -p convex-analytics --test futures_valuation',
        'cargo clippy --locked -p convex-analytics --test futures_valuation --all-features -- -D warnings',
        'cargo run --locked -p convex --example futures_valuation',
        'cargo run --locked -p convex --example futures_valuation --no-default-features',
        'cargo run --locked -p convex --example futures_valuation --features schemars',
        'python3 tests/scripts/rate_calibration_reference.py check',
        'cargo test --locked -p convex-analytics --test rate_calibration',
        'cargo clippy --locked -p convex-analytics --test rate_calibration --all-features -- -D warnings',
        'cargo test --locked -p convex-curves -p convex-rates -p convex-math',
        'cargo run --locked -p convex --example rate_calibration',
        'cargo run --locked -p convex --example rate_calibration --no-default-features',
        'cargo run --locked -p convex --example rate_calibration --features schemars',
        'cargo check --locked -p convex -p convex-core -p convex-math -p convex-curves -p convex-bonds -p convex-rates -p convex-credit -p convex-analytics -p convex-portfolio --no-default-features',
        'cargo test --locked -p convex-math -p convex-core -p convex-ffi --all-features',
        'python3 tests/scripts/cash_calibration_reference.py check',
        'cargo test --locked -p convex-analytics --test cash_calibration',
        'cargo clippy --locked -p convex-curves -p convex-credit -p convex-analytics --all-targets --all-features -- -D warnings',
        'cargo test --locked -p convex-curves -p convex-analytics -p convex-credit',
        'cargo run --locked -p convex --example cash_calibration',
        'cargo run --locked -p convex --example cash_calibration --no-default-features',
        'cargo run --locked -p convex --example cash_calibration --features schemars',
        'python3 tests/scripts/quote_risk_reference.py check',
        'cargo test --locked -p convex-analytics --test quote_risk',
        'cargo test --locked -p convex-portfolio --test signed_risk',
        'cargo clippy --locked -p convex-curves -p convex-analytics -p convex-portfolio -p convex --all-targets --all-features -- -D warnings',
        'cargo run --locked -p convex --example quote_risk',
        'cargo run --locked -p convex --example quote_risk --no-default-features',
        'cargo run --locked -p convex --example quote_risk --features schemars',
        'cargo run --locked -p convex-portfolio --example signed_risk',
        'python3 tests/scripts/lifecycle_reference.py check',
        'cargo test --locked -p convex-analytics -p convex --test lifecycle',
        'cargo test --locked -p convex-bonds -p convex-rates -p convex-credit -p convex-analytics',
        'cargo run --locked -p convex --example lifecycle',
        'cargo run --locked -p convex --example lifecycle --no-default-features',
        'cargo run --locked -p convex --example lifecycle --features schemars',
        'cargo clippy --locked -p convex-core --lib --all-features -- -D warnings',
        'cargo clippy --locked -p convex-bonds -p convex-rates -p convex-credit -p convex-analytics -p convex --all-targets --all-features -- -D warnings',
        'python3 tests/scripts/standalone_reference.py check',
        'cargo test --locked -p convex-analytics --test standalone_workflow',
        'cargo test --locked -p convex --test standalone_book',
        'cargo test --locked -p convex -p convex-analytics -p convex-rates -p convex-credit',
        'cargo run --locked -p convex --example standalone_book',
        'cargo run --locked -p convex --example standalone_book --no-default-features',
        'cargo run --locked -p convex --example standalone_book --features schemars',
        'python3 tests/scripts/monotone_reference.py --check',
        'python3 tests/scripts/calibration_replay_inputs.py --check',
        'python3 tests/scripts/government_inverse_reference.py',
        'python3 tests/scripts/government_inverse_admitted_reference.py',
        'python3 tests/scripts/government_inverse_protocol.py',
        'cargo test --locked --all-features -p convex --test government_inverse -p convex-ffi --test government_inverse -p convex-mcp --test government_inverse -p convex-wasm --test government_inverse -- --test-threads=1',
        'cargo run --locked -p convex --example government_inverse',
        'cargo run --locked -p convex --example government_inverse --no-default-features',
        'cargo run --locked -p convex --example government_inverse --features schemars',
        '/tmp/convex-library-schema/bin/python tests/scripts/government_inverse_schema.py --binary target/debug/examples/government_inverse',
        'python3 tests/scripts/icma_government_reference.py',
        'python3 tests/scripts/icma_helpers_reference.py',
        'python3 tests/scripts/icma_helpers_qualification.py',
        'python3 tests/scripts/icma_helpers_boundaries.py',
        'python3 tests/scripts/icma_helpers_api.py',
        'python3 tests/scripts/icma_helpers_terms.py',
        'python3 tests/scripts/icma_helpers_admission.py',
        'python3 tests/scripts/icma_helpers_failures.py',
        'python3 tests/scripts/icma_helpers_compatibility.py',
        'cargo test --locked --all-features -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test icma_helpers -- --test-threads=1',
        'cargo test --locked --all-features -p convex-engine --test icma_helpers_snapshot',
        'cargo run --locked -p convex --example icma_helpers',
        'cargo run --locked -p convex --example icma_helpers --no-default-features',
        'cargo run --locked -p convex --example icma_helpers --features schemars',
        '/tmp/convex-library-schema/bin/python tests/scripts/icma_helpers_schema.py --binary target/debug/examples/icma_helpers',
        'python3 tests/scripts/icma_helpers_parity.py --binary target/debug/examples/icma_helpers --library target/debug/libconvex_ffi.so',
        'python3 tests/scripts/graph_hedge_reference.py',
        'python3 tests/scripts/graph_hedge_problem_reference.py',
        'python3 tests/scripts/graph_hedge_admission.py',
        'python3 tests/scripts/graph_hedge_roles.py',
        'python3 tests/scripts/graph_hedge_terms.py',
        'python3 tests/scripts/graph_hedge_cash_reference.py',
        'python3 tests/scripts/graph_hedge_protocol.py',
        'python3 tests/scripts/graph_hedge_limits.py',
        'python3 tests/scripts/graph_hedge_census.py',
        'cargo test --locked --all-features -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test graph_hedge -- --test-threads=1',
        'cargo run --locked -p convex --example graph_hedge',
        'cargo run --locked -p convex --example graph_hedge --no-default-features',
        'cargo run --locked -p convex --example graph_hedge --features schemars',
        '/tmp/convex-library-schema/bin/python tests/scripts/graph_hedge_schema.py --binary target/debug/examples/graph_hedge',
        'python3 tests/scripts/graph_hedge_parity.py --binary target/debug/examples/graph_hedge --library target/debug/libconvex_ffi.so',
        'python3 tests/scripts/cash_basis_hedge_reference.py',
        'python3 tests/scripts/cash_basis_hedge_rebuild.py',
        'python3 tests/scripts/cash_basis_hedge_admission.py',
        'python3 tests/scripts/cash_basis_hedge_precision.py',
        'python3 tests/scripts/cash_basis_hedge_book_reference.py',
        'python3 tests/scripts/cash_basis_hedge_census.py',
        'python3 tests/scripts/recovery_hedge_reference.py',
        'python3 tests/scripts/causal_pnl_reference.py',
        'python3 tests/scripts/causal_pnl_ready.py',
        'python3 tests/scripts/causal_pnl_admitted.py',
        'python3 tests/scripts/causal_pnl_bindings.py',
        'python3 tests/scripts/causal_pnl_boundaries.py',
        'python3 tests/scripts/causal_pnl_qualification.py',
        'python3 tests/scripts/causal_pnl_wire.py',
        'python3 tests/scripts/causal_pnl_terms.py',
        'python3 tests/scripts/causal_pnl_publications.py',
        'python3 tests/scripts/causal_pnl_identity.py',
        'python3 tests/scripts/causal_pnl_revisions.py',
        'python3 tests/scripts/causal_pnl_census.py',
        'python3 tests/scripts/causal_pnl_cash_contracts.py',
        'python3 tests/scripts/causal_pnl_credit_dates.py',
        'python3 tests/scripts/causal_pnl_fixing_cutoffs.py',
        'python3 tests/scripts/causal_pnl_policies.py',
        'python3 tests/scripts/causal_pnl_payment_dates.py',
        'python3 tests/scripts/causal_pnl_notice_dates.py',
        'python3 tests/scripts/causal_pnl_amendments.py',
        'python3 tests/scripts/causal_pnl_reconciled.py',
        'python3 tests/scripts/causal_pnl_contexts.py',
        'python3 tests/scripts/causal_pnl_economic_cutoffs.py',
        'cargo test --locked --all-features -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test causal_pnl -- --test-threads=1',
        'cargo test --locked --all-features -p convex --test causal_pnl_boundaries -- --test-threads=1',
        'cargo run --locked -p convex --example causal_pnl',
        'cargo run --locked -p convex --example causal_pnl --no-default-features',
        'cargo run --locked -p convex --example causal_pnl --features schemars',
        '/tmp/convex-library-schema/bin/python tests/scripts/causal_pnl_schema.py --binary target/debug/examples/causal_pnl',
        'python3 tests/scripts/causal_pnl_parity.py --binary target/debug/examples/causal_pnl --library target/debug/libconvex_ffi.so',
        'python3 tests/scripts/cohort_pnl_reference.py',
        'python3 tests/scripts/cohort_pnl_states.py',
        'python3 tests/scripts/cohort_pnl_wire.py',
        'python3 tests/scripts/cohort_pnl_extended.py',
        'python3 tests/scripts/cohort_pnl_contracts.py',
        'python3 tests/scripts/cohort_pnl_bindings.py',
        'python3 tests/scripts/cohort_pnl_currencies.py',
        'python3 tests/scripts/cohort_pnl_revisions.py',
        'python3 tests/scripts/cohort_pnl_cash_dates.py',
        'python3 tests/scripts/cohort_pnl_ownership.py',
        'python3 tests/scripts/cohort_pnl_calendars.py',
        'python3 tests/scripts/cohort_pnl_census.py',
        'python3 tests/scripts/cohort_pnl_full_mixed.py',
        'python3 tests/scripts/cohort_pnl_complete_census.py',
        'python3 tests/scripts/cohort_pnl_full_wire.py',
        'python3 tests/scripts/cohort_pnl_admitted_census.py',
        'cargo test --locked --all-features -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test cohort_pnl -- --test-threads=1',
        'cargo test --locked --all-features -p convex --test cohort_pnl_boundaries -- --test-threads=1',
        'cargo run --locked -p convex --example cohort_pnl',
        'cargo run --locked -p convex --example cohort_pnl --no-default-features',
        'cargo run --locked -p convex --example cohort_pnl --features schemars',
        '/tmp/convex-library-schema/bin/python tests/scripts/cohort_pnl_admitted_schema.py --binary target/debug/examples/cohort_pnl',
        'python3 tests/scripts/cohort_pnl_admitted_parity.py --binary target/debug/examples/cohort_pnl --library target/debug/libconvex_ffi.so',
        'python3 tests/scripts/lot_pnl_reference.py',
        'python3 tests/scripts/lot_pnl_future.py',
        'python3 tests/scripts/lot_pnl_extended.py',
        'python3 tests/scripts/lot_pnl_controls.py',
        'python3 tests/scripts/lot_pnl_chronology.py',
        'python3 tests/scripts/lot_pnl_census.py',
        'cargo test --locked --all-features -p convex-analytics --test amendments_splits -- --test-threads=1',
        'cargo test --locked --all-features -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test lot_pnl -- --test-threads=1',
        'cargo run --locked -p convex --example lot_pnl',
        'cargo run --locked -p convex --example lot_pnl --no-default-features',
        'cargo run --locked -p convex --example lot_pnl --features schemars',
        '/tmp/convex-library-schema/bin/python tests/scripts/lot_pnl_schema.py --binary target/debug/examples/lot_pnl',
        'python3 tests/scripts/lot_pnl_parity.py --binary target/debug/examples/lot_pnl --library target/debug/libconvex_ffi.so',
        'python3 tests/scripts/residual_pnl_reference.py',
        'python3 tests/scripts/residual_pnl_units.py',
        'python3 tests/scripts/residual_pnl_guards.py',
        'python3 tests/scripts/residual_pnl_canonical.py',
        'python3 tests/scripts/residual_pnl_census.py',
        'cargo test --locked --all-features -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test residual_pnl -- --test-threads=1',
        'cargo test --locked --all-features -p convex-analytics --test residual_receipts -- --test-threads=1',
        'cargo run --locked -p convex --example residual_pnl',
        'cargo run --locked -p convex --example residual_pnl --no-default-features',
        'cargo run --locked -p convex --example residual_pnl --features schemars',
        '/tmp/convex-library-schema/bin/python tests/scripts/residual_pnl_schema.py --binary target/debug/examples/residual_pnl',
        'python3 tests/scripts/residual_pnl_parity.py --binary target/debug/examples/residual_pnl --library target/debug/libconvex_ffi.so',
        'python3 tests/scripts/mixed_scenario_reference.py',
        'python3 tests/scripts/mixed_scenario_admission.py',
        'python3 tests/scripts/mixed_scenario_publications.py',
        'python3 tests/scripts/mixed_scenario_boundaries.py',
        'python3 tests/scripts/mixed_scenario_census.py',
        'cargo test --locked --all-features -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test mixed_scenario -- --test-threads=1',
        'cargo test --locked --all-features -p convex --test mixed_scenario_boundaries -- --test-threads=1',
        'cargo run --locked -p convex --example mixed_scenario',
        'cargo run --locked -p convex --example mixed_scenario --no-default-features',
        'cargo run --locked -p convex --example mixed_scenario --features schemars',
        '/tmp/convex-library-schema/bin/python tests/scripts/mixed_scenario_schema.py --binary target/debug/examples/mixed_scenario',
        'python3 tests/scripts/mixed_scenario_parity.py --binary target/debug/examples/mixed_scenario --library target/debug/libconvex_ffi.so',
        'python3 tests/scripts/delivery_hedge_reference.py',
        'python3 tests/scripts/delivery_hedge_qualification.py',
        'python3 tests/scripts/delivery_hedge_admission.py',
        'python3 tests/scripts/delivery_hedge_book.py',
        'python3 tests/scripts/delivery_hedge_ready.py',
        'python3 tests/scripts/delivery_hedge_final.py',
        'python3 tests/scripts/delivery_hedge_errors.py',
        'python3 tests/scripts/delivery_hedge_book_admission.py',
        'python3 tests/scripts/delivery_hedge_decimal.py',
        'python3 tests/scripts/delivery_hedge_rebuild.py',
        'python3 tests/scripts/delivery_hedge_frontier.py',
        'python3 tests/scripts/delivery_hedge_census.py',
        'cargo test --locked --all-features -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test delivery_hedge -- --test-threads=1',
        'cargo test --locked --all-features -p convex --test delivery_boundaries -- --test-threads=1',
        'cargo run --locked -p convex --example delivery_hedge',
        'cargo run --locked -p convex --example delivery_hedge --no-default-features',
        'cargo run --locked -p convex --example delivery_hedge --features schemars',
        '/tmp/convex-library-schema/bin/python tests/scripts/delivery_hedge_schema.py --binary target/debug/examples/delivery_hedge',
        'python3 tests/scripts/delivery_hedge_parity.py --binary target/debug/examples/delivery_hedge --library target/debug/libconvex_ffi.so',
        'python3 tests/scripts/optional_graph_hedge_reference.py',
        'python3 tests/scripts/optional_graph_hedge_admission.py',
        'python3 tests/scripts/optional_graph_hedge_book_binding.py',
        'python3 tests/scripts/optional_graph_hedge_book_policy.py',
        'python3 tests/scripts/optional_graph_hedge_boundaries.py',
        'python3 tests/scripts/optional_graph_hedge_qualification.py',
        'python3 tests/scripts/optional_graph_hedge_resolution.py',
        'python3 tests/scripts/optional_graph_hedge_census.py',
        'python3 tests/scripts/optional_graph_hedge_calibrated.py',
        'python3 tests/scripts/optional_graph_hedge_final_census.py',
        'cargo test --locked --all-features -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test optional_graph_hedge -- --test-threads=1',
        'cargo test --locked --all-features -p convex --test optional_graph_boundaries -- --test-threads=1',
        'cargo run --locked -p convex --example optional_graph_hedge',
        'cargo run --locked -p convex --example optional_graph_hedge --no-default-features',
        'cargo run --locked -p convex --example optional_graph_hedge --features schemars',
        '/tmp/convex-library-schema/bin/python tests/scripts/optional_graph_hedge_schema.py --binary target/debug/examples/optional_graph_hedge',
        'python3 tests/scripts/optional_graph_hedge_parity.py --binary target/debug/examples/optional_graph_hedge --library target/debug/libconvex_ffi.so',
        'python3 tests/scripts/recovery_hedge_admission.py',
        'python3 tests/scripts/recovery_hedge_boundaries.py',
        'python3 tests/scripts/recovery_hedge_census.py',
        'python3 tests/scripts/recovery_hedge_limits.py',
        'cargo test --locked --all-features -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test recovery_hedge -- --test-threads=1',
        'cargo test --locked --all-features -p convex --test recovery_graph -p convex-credit --test recovery_binding -- --test-threads=1',
        'cargo run --locked -p convex --example recovery_hedge',
        'cargo run --locked -p convex --example recovery_hedge --no-default-features',
        'cargo run --locked -p convex --example recovery_hedge --features schemars',
        '/tmp/convex-library-schema/bin/python tests/scripts/recovery_hedge_schema.py --binary target/debug/examples/recovery_hedge',
        'python3 tests/scripts/recovery_hedge_parity.py --binary target/debug/examples/recovery_hedge --library target/debug/libconvex_ffi.so',
        'cargo test --locked --all-features -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test cash_basis_hedge -- --test-threads=1',
        'cargo run --locked -p convex --example cash_basis_hedge',
        'cargo run --locked -p convex --example cash_basis_hedge --no-default-features',
        'cargo run --locked -p convex --example cash_basis_hedge --features schemars',
        '/tmp/convex-library-schema/bin/python tests/scripts/cash_basis_hedge_schema.py --binary target/debug/examples/cash_basis_hedge',
        'python3 tests/scripts/cash_basis_hedge_parity.py --binary target/debug/examples/cash_basis_hedge --library target/debug/libconvex_ffi.so',
        'python3 tests/scripts/icma_risk_reference.py',
        'python3 tests/scripts/icma_risk_admission.py',
        'python3 tests/scripts/icma_risk_failures.py',
        'python3 tests/scripts/icma_risk_protocol.py',
        'python3 tests/scripts/icma_risk_api.py',
        'python3 tests/scripts/icma_risk_owner.py',
        'cargo test --locked --all-features -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test icma_risk -- --test-threads=1',
        'cargo run --locked -p convex --example icma_risk',
        'cargo run --locked -p convex --example icma_risk --no-default-features',
        'cargo run --locked -p convex --example icma_risk --features schemars',
        '/tmp/convex-library-schema/bin/python tests/scripts/icma_risk_schema.py --binary target/debug/examples/icma_risk',
        'python3 tests/scripts/icma_risk_parity.py --binary target/debug/examples/icma_risk --library target/debug/libconvex_ffi.so',
        'python3 tests/scripts/icma_workout_reference.py',
        'python3 tests/scripts/icma_workout_qualification.py',
        'python3 tests/scripts/icma_government_qualification.py',
        'python3 tests/scripts/icma_government_entitlement_reference.py',
        'python3 tests/scripts/icma_government_api_corpus.py',
        'cargo test --locked --all-features -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test icma_government -- --test-threads=1',
        'cargo test --locked --all-features -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test icma_workout -- --test-threads=1',
        'cargo run --locked -p convex --example icma_government',
        'cargo run --locked -p convex --example icma_workout',
        'cargo run --locked -p convex --example icma_government --no-default-features',
        'cargo run --locked -p convex --example icma_workout --no-default-features',
        'cargo run --locked -p convex --example icma_government --features schemars',
        'cargo run --locked -p convex --example icma_workout --features schemars',
        '/tmp/convex-library-schema/bin/python tests/scripts/icma_government_schema.py --binary target/debug/examples/icma_government',
        '/tmp/convex-library-schema/bin/python tests/scripts/icma_workout_schema.py --binary target/debug/examples/icma_workout',
        'python3 tests/scripts/icma_government_parity.py --binary target/debug/examples/icma_government --library target/debug/libconvex_ffi.so',
        'python3 tests/scripts/icma_workout_parity.py --binary target/debug/examples/icma_workout --library target/debug/libconvex_ffi.so',
        'python3 tests/scripts/icma_reference.py',
        'python3 tests/scripts/icma_qualification.py',
        'python3 tests/scripts/icma_cash_components.py',
        'python3 tests/scripts/icma_entitlement_reference.py',
        'python3 tests/scripts/icma_api_corpus.py',
        'cargo test --locked --all-features -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test icma -- --test-threads=1',
        'cargo run --locked -p convex --example icma_quotation',
        'cargo run --locked -p convex --example icma_quotation --no-default-features',
        'cargo run --locked -p convex --example icma_quotation --features schemars',
        '/tmp/convex-library-schema/bin/python tests/scripts/icma_schema.py --binary target/debug/examples/icma_quotation',
        'python3 tests/scripts/icma_parity.py --binary target/debug/examples/icma_quotation --library target/debug/libconvex_ffi.so',
        'python3 tests/scripts/floating_workout_reference.py check',
        'python3 tests/scripts/floating_workout_admission.py check',
        'python3 tests/scripts/floating_workout_qualified.py check',
        'cargo test --locked --all-features -p convex --test floating_workout -p convex-ffi --test floating_workout -p convex-mcp --test floating_workout -p convex-wasm --test floating_workout -- --test-threads=1',
        'cargo run --locked -p convex --example floating_workout',
        'cargo run --locked -p convex --example floating_workout --no-default-features',
        'cargo run --locked -p convex --example floating_workout --features schemars',
        '/tmp/convex-library-schema/bin/python tests/scripts/floating_workout_schema.py --binary target/debug/examples/floating_workout',
        'cargo build --locked -p convex-ffi --lib',
        'python3 tests/scripts/floating_workout_parity.py --binary target/debug/examples/floating_workout --library target/debug/libconvex_ffi.so',
        'python3 tests/scripts/constrained_hedge_reference.py',
        'python3 tests/scripts/constrained_hedge_admission.py',
        'python3 tests/scripts/constrained_hedge_lots.py',
        'python3 tests/scripts/constrained_hedge_protocol.py',
        'cargo test --locked --all-features -p convex --test constrained_hedge -p convex-ffi --test constrained_hedge -p convex-mcp --test constrained_hedge -p convex-wasm --test constrained_hedge -- --test-threads=1',
        'cargo run --locked -p convex --example constrained_hedge',
        'cargo run --locked -p convex --example constrained_hedge --no-default-features',
        'cargo run --locked -p convex --example constrained_hedge --features schemars',
        '/tmp/convex-library-schema/bin/python tests/scripts/constrained_hedge_schema.py --binary target/debug/examples/constrained_hedge',
        'python3 tests/scripts/product_ufr_reference.py --check',
        'python3 tests/scripts/product_ufr_admitted_reference.py --check',
        'cargo test --locked -p convex -p convex-analytics -p convex-engine -p convex-ffi -p convex-mcp -p convex-wasm --all-features --test product_ufr -- --test-threads=1',
        'cargo run --locked -p convex --example product_ufr',
        'cargo run --locked -p convex --example product_ufr --no-default-features',
        'cargo run --locked -p convex --example product_ufr --features schemars',
        '/tmp/convex-library-schema/bin/python tests/scripts/product_ufr_schema.py --binary target/debug/examples/product_ufr',
        'cargo run --locked -p convex-engine --example product_ufr_snapshot -- /tmp/convex-product-ufr-snapshots flat_forward.rate.positive',
        'python3 tests/scripts/product_parametric_reference.py --check',
        'python3 tests/scripts/product_parametric_admitted_reference.py --check',
        'python3 tests/scripts/product_parametric_scaled_reference.py --check',
        'python3 tests/scripts/product_parametric_strict_reference.py --check',
        'python3 tests/scripts/product_parametric_conditioned_reference.py --check',
        'python3 tests/scripts/product_parametric_starts_reference.py --check',
        'cargo test --locked -p convex -p convex-analytics -p convex-engine -p convex-ffi -p convex-mcp -p convex-wasm --all-features --test product_parametric -- --test-threads=1',
        'cargo run --locked -p convex --example product_parametric',
        'cargo run --locked -p convex --example product_parametric --no-default-features',
        'cargo run --locked -p convex --example product_parametric --features schemars',
        '/tmp/convex-library-schema/bin/python tests/scripts/product_parametric_schema.py --binary target/debug/examples/product_parametric',
        'cargo run --locked -p convex-engine --example product_parametric_snapshot -- /tmp/convex-product-parametric-snapshots nelson_siegel.rate.positive',
        'cargo test --locked -p convex --all-features --test calibration_replay -p convex-engine --test calibration_snapshot_replay --test calibration_snapshot_runtime -p convex-ports --test calibration_snapshots',
        'cargo clippy --locked -p convex -p convex-analytics -p convex-ports -p convex-engine --all-targets --all-features -- -D warnings',
        'cargo run --locked -p convex-engine --example calibration_snapshot_replay -- demo /tmp/convex-calibration-snapshot-replay',
        'cargo build --locked -p convex-engine --example calibration_snapshot_replay -p convex --example library_workflow --features schemars',
        '/tmp/convex-library-schema/bin/python tests/scripts/calibration_replay_schema.py --binary target/debug/examples/calibration_snapshot_replay --native-binary target/debug/examples/library_workflow',
        'python3 tests/scripts/curve_replay_inputs.py --check',
        '/tmp/convex-library-schema/bin/python tests/scripts/curve_replay_schema.py --binary target/debug/examples/curve_snapshot_replay --native-binary target/debug/examples/library_workflow',
        'cargo test --locked -p convex-ports --test curve_recipe_snapshots -p convex-engine --all-features --test curve_recipe_replay --test curve_recipe_runtime -p convex-server --test curve_recipe_context_tests',
        'cargo clippy --locked -p convex-ports -p convex-engine -p convex-server --all-targets --all-features -- -D warnings',
        'cargo run --locked -p convex-engine --example curve_snapshot_replay -- demo /tmp/convex-curve-snapshot-replay',
        'python3 tests/scripts/monotone_math_fixture.py --check',
        'python3 tests/scripts/library_monotone_inputs.py --check',
        'cargo test --locked -p convex-math --test monotone_complete -p convex-curves --test monotone_contracts -p convex --test library_monotone -p convex-ffi --test library_monotone -p convex-mcp --test library_monotone -p convex-wasm --test library_monotone --all-features',
        'cargo clippy --locked -p convex-math --lib --test monotone_complete --all-features -- -D warnings',
        'cargo run --locked -p convex --example library_monotone',
        'cargo run --locked -p convex --example library_monotone --no-default-features',
        'cargo run --locked -p convex --example library_monotone --features schemars',
        '/tmp/convex-library-schema/bin/python tests/scripts/library_monotone_schema.py --binary target/debug/examples/library_monotone',
        'python3 tests/scripts/ufr_reference.py --check',
        'python3 tests/scripts/ufr_math_fixture.py --check',
        'python3 tests/scripts/library_ufr_inputs.py --check',
        'cargo test --locked -p convex-math --test ufr_tail -p convex-curves --test ufr_contracts -p convex --test library_ufr -p convex-ffi --test library_ufr -p convex-mcp --test library_ufr -p convex-wasm --test library_ufr --all-features',
        'cargo clippy --locked -p convex-math --lib --test ufr_tail --all-features -- -D warnings',
        'cargo run --locked -p convex --example library_ufr',
        'cargo run --locked -p convex --example library_ufr --no-default-features',
        'cargo run --locked -p convex --example library_ufr --features schemars',
        '/tmp/convex-library-schema/bin/python tests/scripts/library_ufr_schema.py --binary target/debug/examples/library_ufr',
        'python3 tests/scripts/curve_method_reference.py --check',
        'python3 tests/scripts/library_curve_method_inputs.py --check',
        'cargo test --locked -p convex-curves --all-features --test curve_methods -p convex --test library_curve_methods -p convex-ffi --test library_curve_methods -p convex-mcp --test library_curve_methods -p convex-wasm --test library_curve_methods -p convex-engine --test curve_method_migration',
        'cargo run --locked -p convex --example library_curve_methods',
        'cargo run --locked -p convex --example library_curve_methods --no-default-features',
        'cargo run --locked -p convex --example library_curve_methods --features schemars',
        '/tmp/convex-library-schema/bin/python tests/scripts/library_curve_methods_schema.py --binary target/debug/examples/library_curve_methods',
        'python3 tests/scripts/curve_family_reference.py check',
        'python3 tests/scripts/library_curve_family_inputs.py',
        'cargo test --locked -p convex-curves --all-features --test curve_families -p convex --test library_curve_families -p convex-ffi --test library_curve_families -p convex-mcp --test library_curve_families -p convex-wasm --test library_curve_families',
        'cargo run --locked -p convex --example library_curve_families',
        'cargo run --locked -p convex --example library_curve_families --no-default-features',
        'cargo run --locked -p convex --example library_curve_families --features schemars',
        '/tmp/convex-library-schema/bin/python tests/scripts/library_curve_families_schema.py --binary target/debug/examples/library_curve_families',
        'python3 tools/library/api.py',
        'cargo test --locked -p convex --all-features --test library -p convex-ffi --test library -p convex-mcp --test library -p convex-wasm --test library',
        'cargo test --locked -p convex-ffi -p convex-wasm -p convex-mcp',
        'cargo run --locked -p convex --example library_workflow',
        'cargo run --locked -p convex --example library_workflow --no-default-features',
        'cargo run --locked -p convex --example library_workflow --features schemars',
        'python3 -m venv /tmp/convex-library-schema',
        '/tmp/convex-library-schema/bin/python -m pip install -r tools/library/requirements-schema.txt',
        '/tmp/convex-library-schema/bin/python tests/scripts/library_schema.py --binary target/debug/examples/library_workflow',
        'python3 tests/scripts/library_operations_inputs.py',
        'cargo test --locked -p convex --all-features --test library_operations -p convex-ffi --test library_operations -p convex-mcp --test library_operations -p convex-wasm --test library_operations',
        'cargo run --locked -p convex --example library_operations',
        'cargo run --locked -p convex --example library_operations --no-default-features',
        'cargo run --locked -p convex --example library_operations --features schemars',
        '/tmp/convex-library-schema/bin/python tests/scripts/library_operations_schema.py --binary target/debug/examples/library_operations',
        'python3 tests/scripts/cash_quotation_reference.py',
        'cargo test --locked -p convex --all-features --test library_cash -p convex-ffi --test library_cash -p convex-mcp --test library_cash -p convex-wasm --test library_cash',
        'cargo run --locked -p convex --example library_cash',
        'cargo run --locked -p convex --example library_cash --no-default-features',
        'cargo run --locked -p convex --example library_cash --features schemars',
        '/tmp/convex-library-schema/bin/python tests/scripts/library_cash_schema.py --binary target/debug/examples/library_cash',
        'cargo test --locked -p convex-analytics -p convex-portfolio -p convex-curves',
        'cargo test --locked -p convex-curves -p convex-bonds -p convex-analytics',
        'cargo test --locked -p convex-engine --test graph_regressions',
        'cargo build --locked --profile excel -p convex-ffi',
        'python3 tools/assurance/abi_smoke.py',
    ),
    'test': (
        'cargo test --locked --all-features --workspace -- --test-threads=1',
        'python3 -m unittest discover -s tools/tests -p test_market_data_adapter.py',
        'python3 tools/core_ingestion_workflow.py',
        'python3 -m unittest discover -s tools/tests -p test_market_data_stream.py',
        'python3 tools/core_streaming_workflow.py',
        'python3 -m unittest discover -s tools/tests -p test_streaming_analytics.py',
        'python3 tools/core_live_workflow.py',
        'python3 -m unittest discover -s tools/tests -p test_book_updates.py',
        'python3 tools/core_book_workflow.py',
        'python3 -m unittest discover -s tools/tests -p test_book_events.py',
        'python3 tools/core_event_workflow.py',
    ),
    'production-regressions': ('cargo test --locked -p convex-server --test api_integration_tests production_readiness_ -- --include-ignored --test-threads=1',),
    'fmt': ('cargo fmt --all -- --check',),
    'clippy': ('cargo clippy --locked --all-features --workspace -- -D warnings',),
    'docs': ('cargo doc --locked --no-deps --all-features --workspace',),
    'offline-comparison': (
        'python3 reconciliation/comparison.py check',
        'python3 -m unittest discover -s reconciliation -p "test_comparison.py"',
        'cargo build --locked -p convex-analytics --example offline_compare',
        'python3 reconciliation/comparison.py run --candidate-binary target/debug/examples/offline_compare --report comparison-report.json',
        'python3 -m unittest discover -s tests/scripts -p test_swap_quantlib_compare.py',
        'cargo build --locked -p convex --example swap_comparison',
        'python3 tests/scripts/swap_quantlib_compare.py offline --candidate-binary target/debug/examples/swap_comparison --report swap-comparison-report.json',
        'python3 -m unittest discover -s tests/scripts -p test_isda_cds_compare.py',
        'cargo build --locked -p convex-credit --example cds_comparison',
        'python3 tests/scripts/isda_cds_compare.py offline --candidate-binary target/debug/examples/cds_comparison --report cds-comparison-report.json',
        'python3 -m unittest discover -s tests/scripts -p test_isda_credit_compare.py',
        'cargo build --locked -p convex-analytics --example credit_comparison',
        'python3 tests/scripts/isda_credit_compare.py offline --candidate-binary target/debug/examples/credit_comparison --report credit-comparison-report.json',
    ),
}

COMPARISON_PINS = {
    'reconciliation/comparison.py',
    'reconciliation/comparator-records.json',
    'reconciliation/comparison_templates/ore.json',
    'reconciliation/comparison_templates/isda.json',
    'reconciliation/comparison_templates/exchange.json',
    'tests/fixtures/comparisons/catalog.json',
    'tests/fixtures/comparisons/quantlib-1.40.json',
    'crates/convex-analytics/examples/offline_compare.rs',
    'tests/scripts/isda_cds_compare.py',
    'tests/scripts/isda_cds_capture.c',
    'tests/scripts/isda_credit_compare.py',
    'tests/scripts/isda_credit_capture.c',
    'tests/scripts/isda_credit_capture.py',
    *(f'tests/fixtures/comparisons/credit-isda-1.8.3/{name}' for name in (
        'compiler.stdout', 'inputs.json', 'mixed-inputs.json', 'isda-commands.json',
        'isda-manifest.json', 'isda-reference.json', 'isda-reference.stderr', 'native-control.json')),
    'licenses/ISDA-CDS-Standard-Model-Public-License-1.0.txt',
    *(f'tests/fixtures/comparisons/cds-isda-1.8.3/{name}' for name in (
        'NYM.csv', 'USD_SOFR_20220621.zip', 'compiler.stdout', 'grid.json', 'inputs.json',
        'isda-commands.json', 'isda-manifest.json', 'isda-reference.json', 'isda-reference.stderr')),
}

class Failure(ValueError):
    """An explicit assurance rejection; never rely on Python assertions."""

def require(condition: bool, message: str) -> None:
    if not condition:
        raise Failure(message)

def digest(content: bytes) -> str:
    return hashlib.sha256(content).hexdigest()

def unique_object(pairs):
    out = {}
    for key, value in pairs:
        require(key not in out, f'duplicate JSON key: {key}')
        out[key] = value
    return out

def read_json(path: Path):
    return json.loads(path.read_text(), object_pairs_hook=unique_object)

def git(root: Path, *args: str) -> bytes:
    return subprocess.check_output(['git', *args], cwd=root)

def git_source(root: Path, revision: str, path: str) -> bytes | None:
    process = subprocess.run(['git', 'show', f'{revision}:{path}'], cwd=root, capture_output=True)
    return process.stdout if process.returncode == 0 else None

def git_checkout(root: Path, revision: str, path: str) -> bytes | None:
    # Compare physical pins with Git's declared checkout representation as well
    # as its stored blob. For example *.cs is LF in the index and CRLF on disk.
    process = subprocess.run(['git', 'cat-file', '--filters', f'{revision}:{path}'], cwd=root, capture_output=True)
    return process.stdout if process.returncode == 0 else None

def normal_closure(metadata: dict, root_names=DOMAIN) -> set[str]:
    packages = {p['id']: p for p in metadata['packages']}
    nodes = {n['id']: n for n in metadata['resolve']['nodes']}
    roots = [p['id'] for p in metadata['packages'] if p['name'] in root_names and p['id'] in metadata['workspace_members']]
    require({packages[i]['name'] for i in roots} == set(root_names), 'missing pure-domain root')
    visited = set()
    pending = list(roots)
    while pending:
        identity = pending.pop()
        if identity in visited:
            continue
        visited.add(identity)
        require(identity in nodes, f'unresolved package: {identity}')
        for dependency in nodes[identity]['deps']:
            # Build scripts/proc macros and dev-only test/benchmark dependencies
            # are not dependencies of calculations. Target-specific normal edges
            # remain included; no host-only filtering can hide an async adapter.
            if any(kind['kind'] is None for kind in dependency['dep_kinds']):
                pending.append(dependency['pkg'])
    return {packages[i]['name'] for i in visited}

def check_dependencies(metadata: dict, allowed: set[str]) -> set[str]:
    names = normal_closure(metadata)
    require(not names & FORBIDDEN, f'forbidden pure-domain dependencies: {sorted(names & FORBIDDEN)}')
    require(names <= allowed, f'unreviewed pure-domain dependencies: {sorted(names - allowed)}')
    return names

def check_workflow(text: str) -> None:
    # This is a narrow check for the committed workflow structure, not a YAML
    # parser/security boundary. Exact workflow bytes are also review protected.
    text = '\n'.join(line for line in text.splitlines() if not line.lstrip().startswith('#'))
    jobs = re.split(r'^  ([a-z][\w-]*):\s*$', text, flags=re.M)
    bodies = dict(zip(jobs[1::2], jobs[2::2]))
    for name, commands in REQUIRED_RUNS.items():
        require(name in bodies, f'missing required CI job: {name}')
        body = bodies[name]
        if name == 'assurance':
            guard = ("      - name: Check explicit change impact against the PR base\n"
                "        if: github.event_name == 'pull_request'\n"
                "        env:\n"
                "          BASE_SHA: ${{ github.event.pull_request.base.sha }}\n"
                '        run: python3 tools/assurance/check.py --base "$BASE_SHA"')
            require(body.count(guard) == 1, 'missing/altered PR base review step')
            body = body.replace(guard, '')
        if name == 'offline-comparison':
            artifact = ('      - name: Upload offline comparison report\n'
                '        if: always()\n'
                '        uses: actions/upload-artifact@v7\n'
                '        with:\n'
                '          name: offline-comparison-report\n'
                '          path: comparison-report.json\n'
                '          retention-days: 14')
            require(body.count(artifact) == 1, 'missing/altered comparison artifact upload')
            body = body.replace(artifact, '')
            swap_artifact = ('      - name: Upload supplied swap comparison report\n'
                '        if: always()\n'
                '        uses: actions/upload-artifact@v7\n'
                '        with:\n'
                '          name: supplied-swap-comparison-report\n'
                '          path: |\n'
                '            swap-comparison-report.json\n'
                '            swap-comparison-report-native.stdout\n'
                '            swap-comparison-report-native.stderr\n'
                '          retention-days: 14')
            require(body.count(swap_artifact) == 1, 'missing/altered swap comparison artifact upload')
            body = body.replace(swap_artifact, '')
            cds_artifact = ('      - name: Upload supplied CDS comparison report\n'
                '        if: always()\n'
                '        uses: actions/upload-artifact@v7\n'
                '        with:\n'
                '          name: supplied-cds-comparison-report\n'
                '          path: |\n'
                '            cds-comparison-report.json\n'
                '            cds-comparison-report-native.stdout\n'
                '            cds-comparison-report-native.stderr\n'
                '          retention-days: 14')
            require(body.count(cds_artifact) == 1, 'missing/altered CDS comparison artifact upload')
            body = body.replace(cds_artifact, '')
            credit_artifact = ('      - name: Upload supplied credit comparison report\n'
                '        if: always()\n'
                '        uses: actions/upload-artifact@v7\n'
                '        with:\n'
                '          name: supplied-credit-comparison-report\n'
                '          path: |\n'
                '            credit-comparison-report.json\n'
                '            credit-comparison-report-native.stdout\n'
                '            credit-comparison-report-native.stderr\n'
                '          retention-days: 14')
            require(body.count(credit_artifact) == 1, 'missing/altered credit comparison artifact upload')
            body = body.replace(credit_artifact, '')
        require(not re.search(r'^\s*(if|continue-on-error|paths|paths-ignore):', body, re.M), f'conditional or ignored required job: {name}')
        runs = re.findall(r'^\s+run: (.+)$', body, re.M)
        for command in commands:
            require(runs.count(command) == 1, f'missing/altered required check in {name}: {command}')
        require('||' not in body and '; true' not in body, f'ignored command exit in {name}')
    require('on:\n  push:\n    branches: [main]\n  pull_request:\n    branches: [main]\n' in text, 'required push/PR triggers changed')
    require("RUSTFLAGS: -Dwarnings" in text, 'CI must deny compiler warnings')
    require("RUSTDOCFLAGS: -Dwarnings" in text, 'CI must deny rustdoc warnings')
    require('paths-ignore:' not in text and not re.search(r'^  paths:', text, re.M), 'required CI cannot have path filters')
    require("CONVEX_PRODUCTION_PROBES: '1'" in bodies['production-regressions'], 'production probes must be activated')


def check_exceptions(document: dict, today: dt.date) -> set[str]:
    require(set(document) == {'schema_version', 'exceptions'} and document['schema_version'] == 1, 'invalid exception schema')
    identifiers = set()
    fields = {'id', 'scope', 'rationale', 'mitigations', 'owner', 'reviewed_on', 'expires_on', 'tracking_task'}
    for row in document['exceptions']:
        require(set(row) == fields, 'unknown or missing exception fields')
        require(row['id'] not in identifiers, 'duplicate exception ID')
        identifiers.add(row['id'])
        require(row['scope'] and all(isinstance(p, str) and p.startswith('crates/') and '..' not in Path(p).parts for p in row['scope']), 'invalid exception scope')
        require(all(isinstance(row[k], str) and row[k].strip() for k in ['rationale', 'mitigations', 'owner', 'tracking_task']), 'exception lacks rationale, owner or mitigation')
        reviewed = dt.date.fromisoformat(row['reviewed_on'])
        expires = dt.date.fromisoformat(row['expires_on'])
        require(reviewed <= today < expires, f'expired/future exception: {row["id"]}')
        require(0 < (expires - reviewed).days <= 90, f'exception exceeds 90 days: {row["id"]}')
    return identifiers


def inventory_paths(root: Path) -> set[str]:
    paths = {str(p.relative_to(root)) for crate in ['convex-math', 'convex-ffi'] for p in (root / 'crates' / crate / 'src').rglob('*.rs')}
    paths.update(f'crates/{crate}/src/lib.rs' for crate in DOMAIN)
    paths.update(['crates/convex-core/src/types/date.rs', 'crates/convex-core/src/types/frequency.rs', 'crates/convex-core/src/calendars/mod.rs', 'crates/convex-core/src/calendars/conventions.rs', 'crates/convex-core/src/calendars/dynamic.rs', 'crates/convex-bonds/src/fixings.rs', 'crates/convex-core/src/daycounts/actact.rs', 'crates/convex-core/src/daycounts/act365.rs', 'crates/convex-core/src/contracts/mod.rs', 'crates/convex-curves/src/curves/discrete.rs', 'crates/convex-curves/src/curves/segmented.rs', 'crates/convex-curves/src/curves/delegated.rs', 'crates/convex-curves/src/calibration/global_fit.rs', 'crates/convex-analytics/src/calibration/hw1f.rs', 'crates/convex-bonds/src/options/swaption_hw1f.rs', 'crates/convex-engine/src/calc_graph.rs', 'crates/convex-engine/src/runtime/mod.rs'])
    paths.update(str(p.relative_to(root)) for p in (root / 'crates/convex-core/src/cashflows').glob('*.rs'))
    paths.update(str(p.relative_to(root)) for p in (root / 'crates/convex-core/src/futures').glob('*.rs'))
    paths.update(str(p.relative_to(root)) for p in (root / 'crates/convex-analytics/src/futures').glob('*.rs'))
    paths.update(str(p.relative_to(root)) for p in (root / 'crates/convex-analytics/src/rate_calibration').glob('*.rs'))
    paths.update(str(p.relative_to(root)) for p in (root / 'crates/convex-analytics/src/credit_calibration').glob('*.rs'))
    paths.update(str(p.relative_to(root)) for p in (root / 'crates/convex-analytics/src/cash_calibration').glob('*.rs'))
    paths.add('crates/convex-analytics/src/cash_quotation.rs')
    paths.update(str(p.relative_to(root)) for p in (root / 'crates/convex-analytics/src/cash_quotation').glob('*.rs'))
    paths.update(str(p.relative_to(root)) for p in (root / 'crates/convex-analytics/src/quote_risk').glob('*.rs'))
    paths.update(str(p.relative_to(root)) for p in (root / 'crates/convex-analytics/src/risk/hedging/constrained').glob('*.rs'))
    paths.update(str(p.relative_to(root)) for p in (root / 'crates/convex-analytics/src/workflow').rglob('*.rs'))
    paths.update(str(p.relative_to(root)) for p in (root / 'crates/convex/src/library').glob('*.rs'))
    paths.update(str(p.relative_to(root)) for p in (root / 'crates/convex-analytics/src/lifecycle').glob('*.rs'))
    paths.add('crates/convex-portfolio/src/signed_risk.rs')
    paths.add('crates/convex-core/src/contracts/risk.rs')
    paths.add('crates/convex-core/src/contracts/units.rs')
    paths.update(['crates/convex-ports/src/futures_reference.rs',
                  'crates/convex-ext-file/src/reference_repository.rs',
                  'crates/convex-ext-redb/src/reference_repository.rs'])
    paths.update(f'crates/convex-analytics/src/risk/hedging/{name}.rs'
                 for name in ('types', 'strategies', 'compare', 'narrate'))
    paths.update(['crates/convex-core/src/calendars/checked.rs', 'crates/convex-core/src/fixings.rs', 'crates/convex-core/src/daycounts/thirty360.rs', 'crates/convex-bonds/src/cashflows/schedule.rs', 'crates/convex-bonds/src/instruments/fixed_rate.rs', 'crates/convex-bonds/src/arrc.rs'])
    paths.update(str(p.relative_to(root)) for p in (root / 'crates/convex-curves/src/contracts').glob('*.rs'))
    paths.add('crates/convex-analytics/src/curve_inputs.rs')
    paths.add('crates/convex-analytics/src/yields/treasury_bill.rs')
    paths.add('crates/convex-analytics/src/yields/money_market.rs')
    paths.add('crates/convex-bonds/src/pricing/bill.rs')
    paths.add('crates/convex-bonds/src/pricing/zero.rs')
    paths.add('crates/convex-bonds/src/pricing/yield_scalars.rs')
    paths.add('crates/convex-bonds/src/pricing/yield_cash.rs')
    paths.add('crates/convex-bonds/src/pricing/yield_selection.rs')
    paths.add('crates/convex-analytics/src/yas/invoice.rs')
    paths.add('crates/convex-analytics/src/yas/calculator.rs')
    paths.add('crates/convex-bonds/src/pricing/risk_math.rs')
    paths.add('crates/convex-bonds/src/pricing/mod.rs')
    paths.add('crates/convex-bonds/src/pricing/yield_cash.rs')
    paths.add('crates/convex-bonds/src/traits/analytics.rs')
    paths.add('crates/convex-analytics/src/risk/cash.rs')
    paths.add('crates/convex-analytics/src/risk/calculator.rs')
    paths.add('crates/convex-analytics/src/risk/duration/modified.rs')
    paths.add('crates/convex-analytics/src/risk/duration/effective.rs')
    paths.add('crates/convex-analytics/src/risk/duration/spread_duration.rs')
    paths.add('crates/convex-analytics/src/risk/duration/key_rate.rs')
    paths.add('crates/convex-analytics/src/risk/convexity/mod.rs')
    paths.add('crates/convex-analytics/src/risk/convexity/effective.rs')
    paths.add('crates/convex-analytics/src/risk/dv01.rs')
    paths.add('crates/convex-mcp/src/server.rs')
    paths.add('crates/convex-portfolio/src/analytics/mod.rs')
    paths.add('crates/convex-portfolio/src/stress/scenarios.rs')
    paths.add('crates/convex-server/src/routes.rs')
    paths.add('crates/convex-server/src/access/http.rs')
    paths.add('crates/convex-server/src/analytics.rs')
    paths.update([
        'crates/convex-ports/src/core_workflow.rs',
        'crates/convex-ports/src/fixing.rs',
        'crates/convex-ports/src/storage.rs',
        'crates/convex-ports/src/runtime.rs',
        'crates/convex-engine/src/core_workflow.rs',
        'crates/convex-ext-file/src/snapshot_repository.rs',
        'crates/convex-ext-file/src/fixing_repository.rs',
        'crates/convex-ext-redb/src/fixing_repository.rs',
        'crates/convex-ext-redb/src/curve_store.rs',
        'crates/convex-ext-redb/src/runtime_store.rs',
        'crates/convex-server/src/core_workflow.rs',
        'crates/convex-server/src/main.rs',
        'crates/convex-server/src/lib.rs',
        'crates/convex-server/src/bin/convex-core-inputs.rs',
    ])
    paths.add('demo/frontend/src/App.tsx')
    paths.add('demo/frontend/src/lib/api.ts')
    paths.add('demo/frontend/src/components/PortfolioDemo.tsx')
    paths.add('crates/convex-portfolio/Cargo.toml')
    paths.add('crates/convex-core/src/contracts/json.rs')
    paths.add('crates/convex-analytics/src/risk/duration/mod.rs')
    paths.add('crates/convex-analytics/src/risk/hedging/hedge_ratio.rs')
    paths.add('crates/convex-analytics/src/risk/hedging/portfolio.rs')
    paths.add('crates/convex-analytics/src/risk/var/historical.rs')
    paths.add('crates/convex-analytics/src/risk/var/mod.rs')
    paths.add('crates/convex-analytics/src/risk/var/parametric.rs')
    paths.add('crates/convex-analytics/src/spreads/asw/proceeds.rs')
    paths.add('crates/convex-analytics/src/spreads/gspread.rs')
    paths.add('crates/convex-analytics/src/spreads/ispread.rs')
    paths.add('crates/convex-mcp/src/main.rs')
    paths.add('crates/convex-portfolio/src/analytics/key_rates.rs')
    paths.add('crates/convex-portfolio/src/stress/impact.rs')
    paths.add('tools/reconcile_bench/src/main.rs')
    paths.add('crates/convex-analytics/src/risk/calculator.rs')
    paths.add('crates/convex-analytics/src/risk/profile.rs')
    paths.add('crates/convex-analytics/src/risk/duration/modified.rs')
    paths.add('crates/convex-analytics/src/risk/duration/macaulay.rs')
    paths.add('crates/convex-analytics/src/risk/convexity/analytical.rs')
    paths.add('crates/convex-analytics/src/risk/cash.rs')
    paths.add('crates/convex-analytics/src/risk/mod.rs')
    paths.add('crates/convex-bonds/src/cashflows/accrued.rs')
    paths.add('crates/convex-bonds/src/pricing/reinvestment.rs')
    paths.add('crates/convex-bonds/src/types/identifiers.rs')
    paths.add('crates/convex-bonds/src/pricing/short_date.rs')
    paths.add('crates/convex-analytics/src/yields/short_date.rs')
    paths.add('crates/convex-bonds/src/instruments/zero_coupon.rs')
    paths.add('crates/convex-analytics/src/swap_calibration.rs')
    paths.update(['crates/convex-analytics/src/credit_calibration.rs',
                  'crates/convex-analytics/src/credit_risk.rs'])
    paths.add('crates/convex-engine/src/curve_builder.rs')
    paths.update(['crates/convex-engine/src/calibration_replay.rs', 'crates/convex-ports/src/calibration.rs', 'crates/convex-engine/src/curve_replay.rs', 'crates/convex-engine/src/snapshot.rs',
                  'crates/convex-engine/src/configuration.rs', 'crates/convex-engine/src/runtime/worker.rs',
                  'crates/convex-ports/src/snapshot.rs', 'crates/convex-ports/src/configuration.rs',
                  'crates/convex-ports/src/configuration_schema.rs'])
    paths.update(str(p.relative_to(root)) for p in (root / 'crates/convex-rates/src').glob('*.rs'))
    paths.update(str(p.relative_to(root)) for p in (root / 'crates/convex-credit/src').glob('*.rs'))
    paths.update(str(p.relative_to(root)) for p in (root / 'crates/convex-bonds/src/contracts').glob('*.rs'))
    paths.update(str(p.relative_to(root)) for p in (root / 'crates/convex-bonds/src/options/hw1f').glob('*.rs'))
    paths.update(str(p.relative_to(root)) for p in (root / 'crates/convex-analytics/src/optional_bond').rglob('*.rs'))
    paths.update(['crates/convex-analytics/src/bond_contract.rs',
                  'crates/convex-bonds/src/instruments/floating_rate.rs',
                  'crates/convex-bonds/src/instruments/sinking_fund.rs',
                  'crates/convex-bonds/src/instruments/callable.rs',
                  'crates/convex-bonds/src/instruments/callable_frn.rs',
                  'crates/convex-bonds/src/types/amortization.rs'])
    paths.update(['crates/convex-analytics/src/bond_quote.rs', 'crates/convex-analytics/src/pricing.rs',
                  'crates/convex-analytics/src/spreads/zspread.rs', 'crates/convex-analytics/src/spreads/asw/par_par.rs',
                  'crates/convex-analytics/src/spreads/government_curve.rs', 'crates/convex-bonds/src/traits/bond.rs',
                  'crates/convex-bonds/src/traits/analytics.rs', 'crates/convex-bonds/src/pricing/yield_solver.rs',
                  'crates/convex-engine/src/pricing_router.rs', 'crates/convex-ports/src/output.rs',
                  'crates/convex-ports/src/reference_master.rs', 'crates/convex-server/src/handlers.rs'])
    paths.add('crates/convex-wasm/src/ingress.rs')
    paths.add('crates/convex-bonds/src/pricing/dated_cash.rs')
    paths.add('crates/convex-bonds/src/pricing/discount_time.rs')
    paths.add('crates/convex-bonds/src/pricing/yield_engine.rs')
    paths.add('crates/convex-analytics/src/floating_coupon/scenario.rs')
    paths.add('crates/convex-analytics/src/floating_coupon/derivatives.rs')
    paths.add('crates/convex-analytics/src/floating_coupon/inverse.rs')
    paths.add('crates/convex-analytics/src/floating_coupon/directional.rs')
    paths.add('crates/convex-analytics/src/quote_risk/coupon_directional.rs')
    paths.add('crates/convex-analytics/src/quote_risk/coupon_directional_record.rs')
    paths.add('crates/convex-analytics/src/workflow/coupon_directional_hedge.rs')
    paths.add('crates/convex-analytics/src/risk/hedging/constrained/evidence.rs')
    paths.add('crates/convex-analytics/src/floating_coupon/prepared.rs')
    paths.add('crates/convex-analytics/src/floating_coupon/whole_coupon.rs')
    paths.update(['crates/convex-bonds/src/options/floating_coupon.rs', 'crates/convex-analytics/src/floating_coupon.rs'])
    paths.update(['crates/convex-wasm/src/convert.rs','crates/convex-wasm/src/analyze.rs','crates/convex-wasm/src/pricing.rs','crates/convex-wasm/src/conventions.rs','crates/convex-wasm/src/dto.rs'])
    # Explicit portable-kernel owners; keep the set independent of the manifest.
    paths.update(['crates/convex-analytics/src/risk/convexity/analytical.rs', 'crates/convex-analytics/src/risk/duration/macaulay.rs', 'crates/convex-analytics/src/spreads/discount_margin.rs', 'crates/convex-analytics/src/spreads/oas.rs', 'crates/convex-analytics/src/yas/calculator.rs', 'crates/convex-bonds/src/options/binomial_tree.rs', 'crates/convex-bonds/src/options/models/hull_white.rs', 'crates/convex-bonds/src/options/trinomial_tree.rs', 'crates/convex-bonds/src/types/compounding.rs', 'crates/convex-core/src/traits/mod.rs', 'crates/convex-core/src/types/yield_type.rs', 'crates/convex-curves/src/bumping/key_rate.rs', 'crates/convex-curves/src/bumping/parallel.rs', 'crates/convex-curves/src/bumping/scenario.rs', 'crates/convex-curves/src/conversion.rs', 'crates/convex-curves/src/wrappers/credit_curve.rs', 'crates/convex-curves/src/wrappers/rate_curve.rs', 'crates/convex-wasm/src/bond.rs'])
    paths.add('crates/convex-engine/src/portfolio_analytics.rs')
    paths.add('crates/convex-math/src/lib.rs')
    paths.add('crates/convex-portfolio/src/analytics/credit.rs')
    paths.add('crates/convex-portfolio/src/analytics/liquidity.rs')
    paths.add('crates/convex-portfolio/src/analytics/nav.rs')
    paths.add('crates/convex-portfolio/src/analytics/risk.rs')
    paths.add('crates/convex-portfolio/src/analytics/spreads.rs')
    paths.add('crates/convex-portfolio/src/analytics/summary.rs')
    paths.add('crates/convex-portfolio/src/analytics/yields.rs')
    paths.add('crates/convex-portfolio/src/benchmark/comparison.rs')
    paths.add('crates/convex-portfolio/src/benchmark/tracking.rs')
    paths.add('crates/convex-portfolio/src/bucketing/custom.rs')
    paths.add('crates/convex-portfolio/src/bucketing/maturity.rs')
    paths.add('crates/convex-portfolio/src/bucketing/rating.rs')
    paths.add('crates/convex-portfolio/src/bucketing/sector.rs')
    paths.add('crates/convex-portfolio/src/contribution/attribution.rs')
    paths.add('crates/convex-portfolio/src/contribution/risk.rs')
    paths.add('crates/convex-portfolio/src/error.rs')
    paths.add('crates/convex-portfolio/src/etf/basket.rs')
    paths.add('crates/convex-portfolio/src/etf/nav.rs')
    paths.add('crates/convex-portfolio/src/etf/sec.rs')
    paths.add('crates/convex-portfolio/src/portfolio/portfolio.rs')
    paths.add('crates/convex-portfolio/src/types/cash.rs')
    paths.add('crates/convex-portfolio/src/types/holding.rs')
    paths.add('crates/convex-math/src/decimal.rs')
    paths.add('crates/convex-portfolio/src/analytics/metrics.rs')
    paths.add('crates/convex-portfolio/src/types/config.rs')
    paths.add('crates/convex-analytics/src/dto.rs')
    paths.add('crates/convex-core/src/contracts/numeric_decimal.rs')
    paths.add('crates/convex-ffi/src/ingress.rs')
    paths.add('crates/convex-ffi/src/schemas.rs')
    paths.add('crates/convex-portfolio/src/lib.rs')
    paths.add('demo/frontend/src/hooks/index.ts')
    paths.add('demo/frontend/src/hooks/useBondQuoteStream.ts')
    paths.add('demo/frontend/src/hooks/useEtfStream.ts')
    paths.add('demo/frontend/src/hooks/useWebSocket.ts')
    paths.add('crates/convex-analytics/src/risk/hedging/ctd.rs')
    paths.add('crates/convex-analytics/src/risk/hedging/types.rs')
    paths.add('crates/convex-analytics/src/spreads/benchmark.rs')
    paths.add('crates/convex-analytics/src/spreads/sovereign.rs')
    paths.add('crates/convex-core/src/types/mark.rs')
    paths.add('crates/convex-core/src/types/price.rs')
    paths.add('crates/convex-core/src/types/spread.rs')
    paths.add('crates/convex-curves/src/multicurve/index.rs')
    paths.add('crates/convex-curves/src/term_structure.rs')
    paths.add('crates/convex-ffi/src/dispatch.rs')
    paths.add('crates/convex-ffi/src/lib.rs')
    paths.add('crates/convex-ffi/src/build.rs')
    paths.add('crates/convex-curves/src/bumping/mod.rs')
    paths.add('tools/market_data_adapter.py')
    paths.add('tools/market_data_stream.py')
    paths.add('tools/streaming_analytics.py')
    paths.add('tools/book_updates.py')
    paths.add('tools/book_events.py')
    return paths

def check_inventory(root: Path, document: dict, exception_ids: set[str]) -> None:
    require(document['schema_version'] == 1, 'invalid inventory version')
    rows = document['files']
    require(len(rows) == len({r['path'] for r in rows}), 'duplicate inventory path')
    require({r['path'] for r in rows} == inventory_paths(root), 'missing or unexpected critical inventory source')
    for row in rows:
        require(matches_source_pin(root, row['path'], (root / row['path']).read_bytes(), row['sha256']), f'critical source changed without inventory review: {row["path"]}')
        require(row['invariants'] and row['bounds'] and row['failure_contract'], f'incomplete inventory: {row["path"]}')
        require(set(row['exceptions']) <= exception_ids, f'unregistered exception: {row["path"]}')
        for path in row['tests']:
            require((root / path).is_file(), f'missing inventory test: {path}')
    source = (root / 'crates/convex-ffi/src/lib.rs').read_text()
    symbols = re.findall(r'^pub (?:unsafe )?extern "C" fn (convex_\w+)\(', source, re.M)
    require(set(symbols) == set(document['c_exports']) and len(symbols) == len(document['c_exports']), 'C ABI export inventory changed')


def numeric_content(content: bytes, mode: str) -> bytes:
    require(mode in {'file', 'test_section'}, 'invalid numeric evidence mode')
    if mode == 'test_section':
        marker = b'#[cfg(test)]\nmod tests'
        require(marker in content, 'numeric unit-test section missing')
        return content[content.index(marker):]
    return content

def check_numeric(root: Path, document: dict) -> None:
    require(document['schema_version'] == 1 and document['version'] >= 1, 'invalid quantitative evidence version')
    rows = document['artifacts']
    require(len(rows) == len({r['path'] for r in rows}), 'duplicate quantitative artifact')
    required = {'tests/fixtures/quantlib_reference_tests.json', 'reconciliation/reconcile.py', 'reconciliation/ql_bench.py', '.github/workflows/reconcile.yml'} | COMPARISON_PINS
    require(required <= {r['path'] for r in rows}, 'required comparator/tolerance artifact removed')
    for row in rows:
        content = numeric_content((root / row['path']).read_bytes(), row['mode'])
        require((matches_source_pin(root, row['path'], content, row['sha256']) if row['mode'] == 'file' else digest(content) == row['sha256']), f'quantitative evidence changed without version/review: {row["path"]}')
    source = (root / 'crates/convex-math/src/solvers/mod.rs').read_text()
    for name, value in document['solver_defaults'].items():
        require(re.search(rf'pub const {re.escape(name)}:\s*\w+\s*=\s*{re.escape(value)}\s*;', source) is not None, f'reviewed solver default changed: {name}')
    require(document['solver_defaults'] == {'DEFAULT_TOLERANCE': '1e-10', 'DEFAULT_MAX_ITERATIONS': '100'}, 'default changes require explicit quantitative policy revision')


def protected(path: str) -> bool:
    return (path in {'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', '.gitattributes', 'tools/source_pins.py'} or path.startswith('.github/') or path.startswith('tools/library/') or path.startswith('tools/assurance/') and path != 'tools/assurance/change-reviews.json' or path == 'docs/ASSURANCE.md' or any(path.startswith(f'crates/{crate}/') for crate in (*DOMAIN, 'convex-ffi', 'convex-ports', 'convex-ext-file', 'convex-ext-redb')) or path.startswith('tests/fixtures/') or path.startswith(('tests/scripts/isda_cds_', 'tests/scripts/isda_credit_')) or path in {'tests/scripts/test_isda_cds_compare.py', 'tests/scripts/test_isda_credit_compare.py'} or path.startswith('licenses/') or path.startswith('reconciliation/'))

def check_changes(root: Path, base: str, reviews: dict, numeric: dict, today: dt.date) -> int:
    # Include untracked candidate files: local validation cannot miss new modules.
    tracked = git(root, 'diff', '--name-only', '--no-renames', base, '--').decode().splitlines()
    untracked = git(root, 'ls-files', '--others', '--exclude-standard').decode().splitlines()
    changed = {p for p in tracked + untracked if protected(p)}
    require(reviews['schema_version'] == 1, 'invalid change review schema')
    covered = set()
    for row in reviews['reviews']:
        if row['base_commit'] != base:
            continue
        date = dt.date.fromisoformat(row['reviewed_on'])
        require(date <= today and 0 <= (today-date).days <= 90, 'change review expired/future')
        require(row['actor'].strip() and row['rationale'].strip() and row['tests'], 'change impact lacks actor, rationale or tests')
        for path, sha256 in row['candidate_files'].items():
            if path in changed:
                require(path not in covered, f'duplicate change review: {path}')
                candidate = (root / path).read_bytes() if (root / path).is_file() else b''
                require(matches_source_pin(root, path, candidate, sha256), f'candidate changed after impact review: {path}')
                covered.add(path)
    require(changed == covered, f'unreviewed assurance/numerical changes against {base}: {sorted(changed-covered)}')
    # Refreshing a hash cannot quietly widen or delete old quantitative fixtures.
    # Human identities here are declarations for repository review, not an IdP.
    old_policy = git_source(root, base, 'tools/assurance/numeric-evidence.json')
    if old_policy is not None:
        previous = json.loads(old_policy, object_pairs_hook=unique_object)
        old_rows = {r['path']: r for r in previous['artifacts']}
        new_rows = {r['path']: r for r in numeric['artifacts']}
        changed_pins = any(new_rows.get(path) != row for path, row in old_rows.items())
        if changed_pins:
            require(numeric['version'] > previous['version'], 'quantitative pin revision must increment version')
            approvals = [r for r in reviews['reviews'] if r['base_commit'] == base
                and 'tools/assurance/numeric-evidence.json' in r['candidate_files'] and r.get('quantitative_review')]
            require(approvals, 'quantitative pin removal/refresh needs independent review')
            for row in approvals:
                approval = row['quantitative_review']
                require(approval['reviewer'].strip() and approval['reviewer'] != row['actor']
                    and approval['rationale'].strip() and approval['reference_evidence'], 'pin review needs independent owner/evidence')
    for artifact in numeric['artifacts']:
        old = git_source(root, base, artifact['path'])
        if old is None:
            continue
        current = (root / artifact['path']).read_bytes()
        if numeric_content(old, artifact['mode']) == numeric_content(current, artifact['mode']):
            continue
        checkout = git_checkout(root, base, artifact['path'])
        if checkout is not None and numeric_content(checkout, artifact['mode']) == numeric_content(current, artifact['mode']):
            continue
        approvals = [r for r in reviews['reviews'] if r['base_commit'] == base and artifact['path'] in r['candidate_files'] and r.get('quantitative_review')]
        require(approvals, f'changed quantitative evidence needs owner review: {artifact["path"]}')
        for row in approvals:
            approval = row['quantitative_review']
            require(approval['reviewer'].strip() and approval['reviewer'] != row['actor'] and approval['rationale'].strip() and approval['reference_evidence'], 'quantitative review needs independent owner/evidence')
    return len(changed)


def check_lifecycle_source(source: str) -> None:
    # Architectural regression guard, supplemental to checked normal dependencies,
    # Rust lints and actual pure execution. It does not attest custom callbacks.
    code = re.sub(r'//[^\n]*', '', source)
    forbidden = r'\b(?:SystemTime|Instant|tokio|reqwest|convex_engine|convex_ports|convex_server|OpenOptions|File)\b|std::(?:fs|net|thread|env)|\b(?:Utc|Local)::now\s*\('
    require(re.search(forbidden, code) is None, 'lifecycle must not contain clock, I/O or application dependencies')

def validate(root: Path, base: str | None, metadata: dict | None = None) -> dict:
    today = dt.datetime.now(dt.timezone.utc).date()
    directory = root / 'tools/assurance'
    exceptions = check_exceptions(read_json(directory / 'exceptions.json'), today)
    for crate in DOMAIN:
        source = (root / f'crates/{crate}/src/lib.rs').read_text()
        require('#![forbid(unsafe_code)]' in source, f'pure crate must forbid unsafe: {crate}')
        require('#![deny(unused_must_use)]' in source, f'pure crate must check results: {crate}')
    for crate in ['convex-math', 'convex-ffi']:
        source = (root / f'crates/{crate}/src/lib.rs').read_text()
        scope = re.search(r'#!\[cfg_attr\(\s*not\(test\),\s*deny\(([^)]*)\)\s*\)\]', source)
        for lint in ['clippy::unwrap_used', 'clippy::expect_used', 'clippy::panic']:
            require(scope is not None and lint in scope.group(1), f'missing production scoped lint: {crate}/{lint}')
    cargo = tomllib.loads((root / 'Cargo.toml').read_text())
    require(cargo['profile']['excel']['panic'] == 'unwind', 'host ABI profile must unwind')
    source = (root / 'crates/convex-ffi/src/lib.rs').read_text()
    require('#[cfg(panic = "abort")]' in source and 'compile_error!' in source, 'ABI cannot silently use abort')
    if metadata is None:
        metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--locked', '--format-version', '1', '--all-features'], cwd=root))
    dependency_document = read_json(directory / 'dependencies.json')
    names = check_dependencies(metadata, set(dependency_document['allowed_normal_packages']))
    inventory = read_json(directory / 'inventory.json')
    check_inventory(root, inventory, exceptions)
    scope_by_id = {r['id']: set(r['scope']) for r in read_json(directory / 'exceptions.json')['exceptions']}
    for scope in scope_by_id.values():
        require(all((root / p).is_file() for p in scope), 'exception scope has missing source')
    for row in inventory['files']:
        require(all(row['path'] in scope_by_id[i] for i in row['exceptions']), f'exception assigned outside declared scope: {row["path"]}')
    numeric = read_json(directory / 'numeric-evidence.json')
    check_numeric(root, numeric)
    check_workflow((root / '.github/workflows/ci.yml').read_text())
    for path in (root/'crates/convex-analytics/src/lifecycle').glob('*.rs'):
        check_lifecycle_source(path.read_text())
    for path in (root/'crates/convex/src/library').glob('*.rs'):
        check_lifecycle_source(path.read_text())
    changes = check_changes(root, base, read_json(directory / 'change-reviews.json'), numeric, today) if base else None
    comparison_command = [sys.executable, 'reconciliation/comparison.py', '--root', str(root), 'check']
    if base:
        comparison_command.extend(['--base', base])
    comparison = json.loads(subprocess.check_output(comparison_command, cwd=root))
    coverage_command = [sys.executable, 'tools/library/check.py', '--root', str(root)]
    if base:
        coverage_command.extend(['--base', base])
    coverage = json.loads(subprocess.check_output(coverage_command, cwd=root))
    return {'status': 'passed', 'normal_packages': len(names), 'exceptions': len(exceptions), 'reviewed_changes': changes, 'quantitative_artifacts': len(numeric['artifacts']), 'comparison_packs': comparison['packs'], 'product_captures_pending': len(comparison['pending_product_fixtures']), 'library_required_records': coverage['mandatory_records'], 'library_certified': coverage['certified']}

def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument('--base', help='Git revision for explicit change-impact review; required in PR CI')
    args = parser.parse_args()
    try:
        print(json.dumps(validate(args.root.resolve(), args.base), sort_keys=True))
        return 0
    except (Failure, OSError, ValueError, KeyError, TypeError, subprocess.CalledProcessError) as error:
        print(f'assurance rejected: {error}', file=sys.stderr)
        return 1

if __name__ == '__main__':
    sys.exit(main())
