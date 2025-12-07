import React from 'react';
import { createRoot } from 'react-dom/client';
import App from './App';
import './styles/bloomberg.css';
// WASM initialization fix

// Initialize WASM module
let wasmModule = null;

async function initWasm() {
  try {
    // Dynamic import of the WASM module (web target)
    const wasm = await import('../pkg');
    // With --target web, we need to call the default init function first
    if (wasm.default) {
      await wasm.default();
    }
    wasmModule = wasm;
    console.log('WASM module initialized successfully');
    return wasm;
  } catch (error) {
    console.error('Failed to initialize WASM module:', error);
    // Return mock module for development without WASM
    return createMockModule();
  }
}

// Mock module for development/testing without WASM
function createMockModule() {
  console.warn('Using mock WASM module - build WASM for full functionality');

  return {
    analyze_bond: (params, price, curve) => ({
      clean_price: price,
      dirty_price: price + 1.5,
      accrued_interest: 1.5,
      ytm: 5.25,
      current_yield: 4.8,
      simple_yield: 5.1,
      money_market_yield: 5.0,
      modified_duration: 4.25,
      macaulay_duration: 4.35,
      convexity: 22.5,
      dv01: 0.0425,
      g_spread: 125.5,
      z_spread: 130.2,
      asw_spread: 128.0,
      days_to_maturity: 1825,
      years_to_maturity: 5.0,
      error: null,
    }),
    get_cash_flows: (params) => [
      { date: '2025-06-15', amount: 2.5, cf_type: 'coupon' },
      { date: '2025-12-15', amount: 2.5, cf_type: 'coupon' },
      { date: '2026-06-15', amount: 2.5, cf_type: 'coupon' },
      { date: '2026-12-15', amount: 2.5, cf_type: 'coupon' },
      { date: '2027-06-15', amount: 2.5, cf_type: 'coupon' },
      { date: '2027-12-15', amount: 2.5, cf_type: 'coupon' },
      { date: '2028-06-15', amount: 2.5, cf_type: 'coupon' },
      { date: '2028-12-15', amount: 2.5, cf_type: 'coupon' },
      { date: '2029-06-15', amount: 2.5, cf_type: 'coupon' },
      { date: '2029-12-15', amount: 102.5, cf_type: 'coupon_and_principal' },
    ],
    calculate_accrued: (params) => ({ Ok: 1.5 }),
    calculate_simple_metrics: (params, price) => ({
      clean_price: price,
      dirty_price: price + 1.5,
      accrued_interest: 1.5,
      current_yield: 4.8,
      days_to_maturity: 1825,
      years_to_maturity: 5.0,
    }),
  };
}

// Export for use in components
export function getWasmModule() {
  return wasmModule;
}

// Initialize and render
async function main() {
  const wasm = await initWasm();
  wasmModule = wasm;

  const container = document.getElementById('root');
  const root = createRoot(container);
  root.render(
    <React.StrictMode>
      <App wasmModule={wasm} />
    </React.StrictMode>
  );
}

main();
