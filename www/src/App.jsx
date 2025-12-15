import React, { useState, useCallback, useEffect } from 'react';
import Header from './components/Header';
import BondInput from './components/BondInput';
import ConventionSettings from './components/ConventionSettings';
import YieldAnalysis from './components/YieldAnalysis';
import SpreadAnalysis from './components/SpreadAnalysis';
import RiskMetrics from './components/RiskMetrics';
import CashFlowTable from './components/CashFlowTable';
import PriceYieldChart from './components/PriceYieldChart';
import Benchmark from './components/Benchmark';
import Invoice from './components/Invoice';
import TreasuryCurve from './components/TreasuryCurve';
import CallSchedule from './components/CallSchedule';

// Default bond parameters
const getDefaultBond = () => {
  const today = new Date();
  const settlement = new Date(today);
  settlement.setDate(settlement.getDate() + 2); // T+2

  const maturity = new Date(settlement);
  maturity.setFullYear(maturity.getFullYear() + 5);

  const issue = new Date(settlement);
  issue.setFullYear(issue.getFullYear() - 1);

  return {
    name: 'Sample Corporate Bond 5% 2029',
    couponRate: 5.0,
    maturityDate: maturity.toISOString().split('T')[0],
    issueDate: issue.toISOString().split('T')[0],
    settlementDate: settlement.toISOString().split('T')[0],
    dayCount: '30/360',
    frequency: 2,
    faceValue: 100,
    currency: 'USD',
    firstCouponDate: '',
    callSchedule: [],
    volatility: 1.0, // Interest rate volatility for OAS (%)
  };
};

// Default Treasury curve (approximate rates)
const DEFAULT_TREASURY_CURVE = {
  '1M': 4.30,
  '3M': 4.32,
  '6M': 4.28,
  '1Y': 4.15,
  '2Y': 4.00,
  '3Y': 3.95,
  '5Y': 3.95,
  '7Y': 4.00,
  '10Y': 4.10,
  '20Y': 4.40,
  '30Y': 4.30,
};

// Tenor to years mapping
const TENOR_YEARS = {
  '1M': 1/12,
  '3M': 0.25,
  '6M': 0.5,
  '1Y': 1,
  '2Y': 2,
  '3Y': 3,
  '5Y': 5,
  '7Y': 7,
  '10Y': 10,
  '20Y': 20,
  '30Y': 30,
};

function addYears(date, years) {
  const result = new Date(date);
  result.setFullYear(result.getFullYear() + Math.floor(years));
  result.setMonth(result.getMonth() + Math.round((years % 1) * 12));
  return result.toISOString().split('T')[0];
}

// Generate curve points from Treasury curve object
function generateCurvePoints(treasuryCurve, maturityDate) {
  const baseDate = new Date();
  const maturity = new Date(maturityDate);
  const yearsToMaturity = Math.max(0.5, (maturity - baseDate) / (365.25 * 24 * 60 * 60 * 1000));

  // Filter tenors up to maturity + buffer, keep at least a few points
  const relevantTenors = Object.entries(TENOR_YEARS)
    .filter(([_, years]) => years <= yearsToMaturity + 2)
    .sort((a, b) => a[1] - b[1]);

  if (relevantTenors.length < 2) {
    // Ensure at least 2 points
    return [
      { date: addYears(baseDate, 0.25), rate: treasuryCurve['3M'] || 4.0 },
      { date: addYears(baseDate, 1), rate: treasuryCurve['1Y'] || 4.0 },
    ];
  }

  return relevantTenors.map(([tenor, years]) => ({
    date: addYears(baseDate, years),
    rate: treasuryCurve[tenor] || 4.0,
  }));
}

// Fetch USD Treasury rates from Treasury.gov XML feed
async function fetchUSDRates() {
  const year = new Date().getFullYear();
  const treasuryUrl = `https://home.treasury.gov/resource-center/data-chart-center/interest-rates/pages/xml?data=daily_treasury_yield_curve&field_tdr_date_value=${year}`;

  // Try direct fetch first, fallback to CORS proxy
  let response;
  try {
    response = await fetch(treasuryUrl);
    if (!response.ok) throw new Error('Direct fetch failed');
  } catch {
    const proxyUrl = `https://api.allorigins.win/raw?url=${encodeURIComponent(treasuryUrl)}`;
    response = await fetch(proxyUrl);
  }

  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }

  const xmlText = await response.text();
  const parser = new DOMParser();
  const xmlDoc = parser.parseFromString(xmlText, 'text/xml');

  const entries = xmlDoc.querySelectorAll('entry');
  if (entries.length === 0) throw new Error('No data entries found');

  const latestEntry = entries[entries.length - 1];
  const props = latestEntry.querySelector('content')?.querySelector('properties');
  if (!props) throw new Error('No properties found');

  const rates = {};
  const mappings = {
    'BC_1MONTH': '1M', 'BC_3MONTH': '3M', 'BC_6MONTH': '6M',
    'BC_1YEAR': '1Y', 'BC_2YEAR': '2Y', 'BC_3YEAR': '3Y',
    'BC_5YEAR': '5Y', 'BC_7YEAR': '7Y', 'BC_10YEAR': '10Y',
    'BC_20YEAR': '20Y', 'BC_30YEAR': '30Y',
  };

  for (const [xmlKey, tenorKey] of Object.entries(mappings)) {
    const elem = props.querySelector(xmlKey);
    if (elem?.textContent) {
      const rate = parseFloat(elem.textContent);
      if (!isNaN(rate)) rates[tenorKey] = rate;
    }
  }

  if (Object.keys(rates).length === 0) throw new Error('No rates parsed');
  return rates;
}

// European government bond curves by country
const EUR_CURVE_DEFAULTS = {
  // German Bunds (DBR) - lowest yields, AAA rated
  DBR: {
    '1M': 2.85, '3M': 2.80, '6M': 2.70, '1Y': 2.45,
    '2Y': 2.20, '3Y': 2.15, '5Y': 2.10, '7Y': 2.15,
    '10Y': 2.25, '20Y': 2.45, '30Y': 2.50
  },
  // French OATs - slightly higher than DBR
  OAT: {
    '1M': 2.90, '3M': 2.85, '6M': 2.78, '1Y': 2.55,
    '2Y': 2.35, '3Y': 2.32, '5Y': 2.35, '7Y': 2.45,
    '10Y': 2.85, '20Y': 3.20, '30Y': 3.35
  },
  // Italian BTPs - higher yields due to credit spread
  BTP: {
    '1M': 3.10, '3M': 3.05, '6M': 3.00, '1Y': 2.85,
    '2Y': 2.75, '3Y': 2.80, '5Y': 3.00, '7Y': 3.25,
    '10Y': 3.55, '20Y': 4.00, '30Y': 4.20
  },
  // Spanish Bonos
  SPGB: {
    '1M': 2.95, '3M': 2.90, '6M': 2.82, '1Y': 2.60,
    '2Y': 2.45, '3Y': 2.48, '5Y': 2.55, '7Y': 2.70,
    '10Y': 3.05, '20Y': 3.50, '30Y': 3.70
  },
  // Dutch DSL
  DSL: {
    '1M': 2.87, '3M': 2.82, '6M': 2.72, '1Y': 2.48,
    '2Y': 2.25, '3Y': 2.20, '5Y': 2.18, '7Y': 2.25,
    '10Y': 2.40, '20Y': 2.65, '30Y': 2.75
  },
  // Belgian OLO
  OLO: {
    '1M': 2.92, '3M': 2.87, '6M': 2.77, '1Y': 2.55,
    '2Y': 2.38, '3Y': 2.35, '5Y': 2.40, '7Y': 2.52,
    '10Y': 2.80, '20Y': 3.15, '30Y': 3.30
  },
  // Austrian RAGB
  RAGB: {
    '1M': 2.88, '3M': 2.83, '6M': 2.73, '1Y': 2.50,
    '2Y': 2.28, '3Y': 2.24, '5Y': 2.25, '7Y': 2.35,
    '10Y': 2.55, '20Y': 2.85, '30Y': 2.95
  },
  // Portuguese PGB
  PGB: {
    '1M': 3.00, '3M': 2.95, '6M': 2.88, '1Y': 2.70,
    '2Y': 2.58, '3Y': 2.62, '5Y': 2.75, '7Y': 2.92,
    '10Y': 3.15, '20Y': 3.60, '30Y': 3.80
  },
  // Greek GGB
  GGB: {
    '1M': 3.20, '3M': 3.15, '6M': 3.10, '1Y': 3.00,
    '2Y': 2.95, '3Y': 3.05, '5Y': 3.25, '7Y': 3.50,
    '10Y': 3.75, '20Y': 4.20, '30Y': 4.40
  },
};

// Fetch EUR rates - try ECB first, fallback to defaults
async function fetchEURRates(curveType = 'DBR') {
  // ECB Statistical Data Warehouse - try to get AAA euro area yields
  const baseUrl = 'https://data-api.ecb.europa.eu/service/data/YC/B.U2.EUR.4F.G_N_A.SV_C_YM';
  const proxyUrl = `https://api.allorigins.win/raw?url=${encodeURIComponent(baseUrl + '?lastNObservations=1&format=jsondata')}`;

  try {
    const response = await fetch(proxyUrl);
    if (!response.ok) throw new Error(`HTTP ${response.status}`);

    const data = await response.json();
    const rates = {};

    // ECB provides rates at various maturities - map to our tenors
    const series = data?.dataSets?.[0]?.series || {};
    const dimensions = data?.structure?.dimensions?.series || [];
    const maturityDim = dimensions.find(d => d.id === 'MATURITY_NOT_ISOCODE');

    if (maturityDim) {
      const tenorMap = {
        'Y1': '1Y', 'Y2': '2Y', 'Y3': '3Y', 'Y5': '5Y',
        'Y7': '7Y', 'Y10': '10Y', 'Y20': '20Y', 'Y30': '30Y',
        'M3': '3M', 'M6': '6M', 'M1': '1M'
      };

      for (const [key, seriesData] of Object.entries(series)) {
        const obs = seriesData?.observations;
        if (obs) {
          const latestValue = Object.values(obs).pop()?.[0];
          if (latestValue !== undefined) {
            const keyParts = key.split(':');
            const maturityIdx = parseInt(keyParts[4]);
            const maturity = maturityDim.values[maturityIdx]?.id;
            if (maturity && tenorMap[maturity]) {
              rates[tenorMap[maturity]] = latestValue;
            }
          }
        }
      }
    }

    if (Object.keys(rates).length > 0) {
      // Apply spread adjustment based on curve type (DBR as base)
      const spreads = {
        DBR: 0, OAT: 0.50, BTP: 1.20, SPGB: 0.70, DSL: 0.10,
        OLO: 0.45, RAGB: 0.20, PGB: 0.85, GGB: 1.40
      };
      const spread = (spreads[curveType] || 0) / 100;

      // Add spread to each tenor
      Object.keys(rates).forEach(tenor => {
        rates[tenor] = rates[tenor] + spread * 100;
      });

      return rates;
    }
    throw new Error('No EUR rates parsed');
  } catch (error) {
    console.warn('ECB fetch failed, using defaults:', error);
    return EUR_CURVE_DEFAULTS[curveType] || EUR_CURVE_DEFAULTS.DBR;
  }
}

// Fetch GBP rates from Bank of England
async function fetchGBPRates() {
  // BoE provides gilt yields via their Statistical Interactive Database
  // Using proxy due to CORS
  try {
    const baseUrl = 'https://www.bankofengland.co.uk/boeapps/iadb/fromshowcolumns.asp?csv.x=yes&Datefrom=01/Jan/2024&Dateto=now&SeriesCodes=IUDSOIA,IUDMO02,IUDMO05,IUDMO10,IUDMO15,IUDMO20,IUDMO25,IUDMO30&CSVF=TN&UsingCodes=Y&VPD=Y';
    const proxyUrl = `https://api.allorigins.win/raw?url=${encodeURIComponent(baseUrl)}`;

    const response = await fetch(proxyUrl);
    if (!response.ok) throw new Error(`HTTP ${response.status}`);

    const csvText = await response.text();
    const lines = csvText.trim().split('\n');
    if (lines.length < 2) throw new Error('No data');

    // Get last line (most recent data)
    const lastLine = lines[lines.length - 1];
    const values = lastLine.split(',');

    // Map BoE series to tenors (approximate)
    const rates = {};
    if (values[1]) rates['1Y'] = parseFloat(values[1]);
    if (values[2]) rates['2Y'] = parseFloat(values[2]);
    if (values[3]) rates['5Y'] = parseFloat(values[3]);
    if (values[4]) rates['10Y'] = parseFloat(values[4]);
    if (values[5]) rates['15Y'] = parseFloat(values[5]); // No 15Y in our curve, skip
    if (values[6]) rates['20Y'] = parseFloat(values[6]);
    if (values[7]) rates['30Y'] = parseFloat(values[7]);

    if (Object.keys(rates).length > 0) return rates;
    throw new Error('No GBP rates parsed');
  } catch (error) {
    console.warn('BoE fetch failed, using defaults:', error);
    // Return typical GBP Gilt rates as fallback
    return {
      '1M': 4.65, '3M': 4.60, '6M': 4.50, '1Y': 4.30,
      '2Y': 4.10, '3Y': 4.05, '5Y': 4.00, '7Y': 4.05,
      '10Y': 4.15, '20Y': 4.50, '30Y': 4.45
    };
  }
}

// Fetch JPY JGB rates
async function fetchJPYRates() {
  // Japan MoF doesn't have easy CORS access, use fallback
  return {
    '1M': 0.05, '3M': 0.08, '6M': 0.12, '1Y': 0.30,
    '2Y': 0.55, '3Y': 0.65, '5Y': 0.85, '7Y': 1.00,
    '10Y': 1.10, '20Y': 1.80, '30Y': 2.10
  };
}

// Curve type mapping by currency
const CURVE_TYPES_BY_CURRENCY = {
  USD: ['UST'],
  EUR: ['DBR', 'OAT', 'BTP', 'SPGB', 'DSL', 'OLO', 'RAGB', 'PGB', 'GGB'],
  GBP: ['GILT'],
  JPY: ['JGB'],
  CHF: ['SWISS'],
  AUD: ['ACGB'],
  CAD: ['CAN'],
  NZD: ['NZGB'],
};

const CURVE_TYPE_NAMES = {
  UST: 'US Treasury',
  DBR: 'German Bund',
  OAT: 'French OAT',
  BTP: 'Italian BTP',
  SPGB: 'Spanish Bono',
  DSL: 'Dutch DSL',
  OLO: 'Belgian OLO',
  RAGB: 'Austrian RAGB',
  PGB: 'Portuguese PGB',
  GGB: 'Greek GGB',
  GILT: 'UK Gilt',
  JGB: 'Japan JGB',
  SWISS: 'Swiss Govt',
  ACGB: 'Australian Govt',
  CAN: 'Canadian Govt',
  NZGB: 'NZ Govt',
};

// ============================================================================
// Interest Rate Volatility Fetching
// ============================================================================

// Default volatility assumptions by currency (in %)

// Main function to fetch rates based on currency and curve type
async function fetchRatesForCurrency(currency, curveType) {
  console.log(`Fetching rates for ${currency} (${curveType})...`);

  switch (currency) {
    case 'USD':
      return await fetchUSDRates();
    case 'EUR':
      return await fetchEURRates(curveType || 'DBR');
    case 'GBP':
      return await fetchGBPRates();
    case 'JPY':
      return await fetchJPYRates();
    case 'CHF':
      return {
        '1M': 1.20, '3M': 1.15, '6M': 1.05, '1Y': 0.85,
        '2Y': 0.70, '3Y': 0.65, '5Y': 0.60, '7Y': 0.65,
        '10Y': 0.75, '20Y': 0.85, '30Y': 0.80
      };
    case 'AUD':
      return {
        '1M': 4.25, '3M': 4.20, '6M': 4.10, '1Y': 3.90,
        '2Y': 3.70, '3Y': 3.65, '5Y': 3.80, '7Y': 3.95,
        '10Y': 4.10, '20Y': 4.40, '30Y': 4.35
      };
    case 'CAD':
      return {
        '1M': 3.80, '3M': 3.75, '6M': 3.60, '1Y': 3.40,
        '2Y': 3.20, '3Y': 3.15, '5Y': 3.10, '7Y': 3.15,
        '10Y': 3.25, '20Y': 3.40, '30Y': 3.35
      };
    case 'NZD':
      return {
        '1M': 4.75, '3M': 4.70, '6M': 4.55, '1Y': 4.30,
        '2Y': 4.00, '3Y': 3.95, '5Y': 4.05, '7Y': 4.20,
        '10Y': 4.40, '20Y': 4.60, '30Y': 4.55
      };
    default:
      return DEFAULT_TREASURY_CURVE;
  }
}

// Default conventions
const getDefaultConventions = () => ({
  market: 'US',
  instrumentType: 'GovernmentBond',
  yieldConvention: 'Street',
  compounding: 'SemiAnnual',
  settlementDays: 1,
  exDividendDays: null,
});

function App({ wasmModule }) {
  // Bond parameters
  const [bond, setBond] = useState(getDefaultBond);
  const [price, setPrice] = useState(100.0);
  const [treasuryCurve, setTreasuryCurve] = useState(DEFAULT_TREASURY_CURVE);
  const [curveType, setCurveType] = useState('UST');
  const [isFetchingRates, setIsFetchingRates] = useState(false);
  const [ratesLastUpdated, setRatesLastUpdated] = useState(null);

  // Market conventions
  const [conventions, setConventions] = useState(getDefaultConventions);
  const [conventionsExpanded, setConventionsExpanded] = useState(false);
  const [compoundingLinked, setCompoundingLinked] = useState(true);

  // Invoice face amount
  const [faceAmount, setFaceAmount] = useState(1000000);

  // Analysis results
  const [analysis, setAnalysis] = useState(null);
  const [cashFlows, setCashFlows] = useState([]);
  const [priceYieldData, setPriceYieldData] = useState([]);
  const [isCalculating, setIsCalculating] = useState(false);
  const [error, setError] = useState(null);

  // Calculate bond analytics
  const calculate = useCallback(() => {
    if (!wasmModule) {
      setError('WASM module not loaded');
      return;
    }

    setIsCalculating(true);
    setError(null);

    try {
      // Prepare bond parameters with conventions
      const bondParams = {
        coupon_rate: bond.couponRate,
        maturity_date: bond.maturityDate,
        issue_date: bond.issueDate,
        settlement_date: bond.settlementDate,
        face_value: bond.faceValue,
        frequency: bond.frequency,
        day_count: bond.dayCount,
        currency: bond.currency,
        first_coupon_date: bond.firstCouponDate || null,
        call_schedule: bond.callSchedule && bond.callSchedule.length > 0
          ? bond.callSchedule.map(c => ({ date: c.date, price: c.price }))
          : null,
        volatility: bond.volatility || 1.0, // Interest rate volatility for OAS
        // Convention parameters
        market: conventions.market,
        instrument_type: conventions.instrumentType,
        yield_convention: conventions.yieldConvention,
        compounding: conventions.compounding,
        settlement_days: conventions.settlementDays,
        ex_dividend_days: conventions.exDividendDays,
        use_business_days: true,
      };

      // Generate curve points from Treasury curve
      const curvePoints = generateCurvePoints(treasuryCurve, bond.maturityDate);

      // Call WASM analytics
      const result = wasmModule.analyze_bond(bondParams, price, curvePoints);

      if (result.error) {
        setError(result.error);
        setAnalysis(null);
      } else {
        setAnalysis(result);
        setError(null);
      }

      // Get cash flows
      const flows = wasmModule.get_cash_flows(bondParams);
      setCashFlows(flows || []);

      // Generate price/yield curve data
      generatePriceYieldCurve(bondParams, curvePoints);

    } catch (err) {
      console.error('Calculation error:', err);
      setError(err.message || 'Calculation failed');
    } finally {
      setIsCalculating(false);
    }
  }, [wasmModule, bond, price, treasuryCurve, conventions]);

  // Generate price/yield curve for chart
  const generatePriceYieldCurve = useCallback((bondParams, curvePoints) => {
    if (!wasmModule) return;

    const data = [];
    for (let p = 80; p <= 120; p += 2) {
      try {
        const result = wasmModule.analyze_bond(bondParams, p, curvePoints);
        if (!result.error && result.ytm !== null) {
          data.push({
            price: p,
            yield: result.ytm,
            duration: result.modified_duration,
          });
        }
      } catch (e) {
        // Skip invalid points
      }
    }
    setPriceYieldData(data);
  }, [wasmModule]);

  // Update curve type when currency changes
  useEffect(() => {
    const availableCurves = CURVE_TYPES_BY_CURRENCY[bond.currency] || ['UST'];
    if (!availableCurves.includes(curveType)) {
      setCurveType(availableCurves[0]);
    }
  }, [bond.currency, curveType]);

  // Auto-fetch rates on initial load and when currency/curveType changes
  useEffect(() => {
    const fetchRates = async () => {
      setIsFetchingRates(true);
      try {
        const rates = await fetchRatesForCurrency(bond.currency, curveType);
        setTreasuryCurve(prev => ({ ...prev, ...rates }));
        setRatesLastUpdated(new Date().toLocaleString());
      } catch (err) {
        console.warn(`Failed to fetch ${bond.currency} (${curveType}) rates, using defaults:`, err);
      } finally {
        setIsFetchingRates(false);
      }
    };
    fetchRates();
  }, [bond.currency, curveType]);

  // Auto-calculate on any input change (debounced)
  useEffect(() => {
    // Skip if no WASM module yet
    if (!wasmModule) return;

    const timer = setTimeout(() => {
      calculate();
    }, 300); // 300ms debounce

    return () => clearTimeout(timer);
  }, [wasmModule, bond, price, treasuryCurve, calculate]);

  // Handle bond parameter changes
  const handleBondChange = useCallback((field, value) => {
    setBond(prev => ({ ...prev, [field]: value }));
  }, []);

  // Handle convention changes
  const handleConventionChange = useCallback((field, value) => {
    setConventions(prev => ({ ...prev, [field]: value }));
  }, []);

  // Map bond frequency to compounding value
  const frequencyToCompounding = (freq) => {
    switch (freq) {
      case 1: return 'Annual';
      case 2: return 'SemiAnnual';
      case 4: return 'Quarterly';
      case 12: return 'Monthly';
      default: return 'SemiAnnual';
    }
  };

  // Link compounding to bond frequency - when frequency changes, update compounding (only if linked)
  useEffect(() => {
    if (!compoundingLinked) return;

    const newCompounding = frequencyToCompounding(bond.frequency);
    setConventions(prev => {
      if (prev.compounding !== newCompounding) {
        return { ...prev, compounding: newCompounding };
      }
      return prev;
    });
  }, [bond.frequency, compoundingLinked]);

  // Handle defaults applied from ConventionSettings (for non-compounding fields)
  const handleApplyDefaults = useCallback((defaultData) => {
    // Compounding is linked to bond frequency, so we don't override it from market defaults
    // But we could update bond frequency to match market convention if desired
    // For now, just log that defaults were applied
    console.log('Market defaults applied:', defaultData);
  }, []);

  // Handle price change with auto-calculate
  const handlePriceChange = useCallback((newPrice) => {
    setPrice(newPrice);
  }, []);

  // Handle yield change - calculate price from target yield
  const handleYieldChange = useCallback((targetYtm) => {
    if (!wasmModule) return;

    try {
      const bondParams = {
        coupon_rate: bond.couponRate,
        maturity_date: bond.maturityDate,
        issue_date: bond.issueDate,
        settlement_date: bond.settlementDate,
        face_value: bond.faceValue,
        frequency: bond.frequency,
        day_count: bond.dayCount,
        currency: bond.currency,
        first_coupon_date: bond.firstCouponDate || null,
        call_schedule: null,
      };

      const curvePoints = generateCurvePoints(treasuryCurve, bond.maturityDate);
      const result = wasmModule.price_from_yield(bondParams, targetYtm, curvePoints);

      if (result.clean_price && !result.error) {
        setPrice(result.clean_price);
      } else if (result.error) {
        setError(result.error);
      }
    } catch (err) {
      console.error('Yield solve error:', err);
      setError('Failed to calculate price from yield');
    }
  }, [wasmModule, bond, treasuryCurve]);

  // Handle spread change - calculate price from target Z-spread
  const handleSpreadChange = useCallback((targetSpreadBps) => {
    if (!wasmModule) return;

    try {
      const bondParams = {
        coupon_rate: bond.couponRate,
        maturity_date: bond.maturityDate,
        issue_date: bond.issueDate,
        settlement_date: bond.settlementDate,
        face_value: bond.faceValue,
        frequency: bond.frequency,
        day_count: bond.dayCount,
        currency: bond.currency,
        first_coupon_date: bond.firstCouponDate || null,
        call_schedule: null,
      };

      const curvePoints = generateCurvePoints(treasuryCurve, bond.maturityDate);
      const result = wasmModule.price_from_spread(bondParams, targetSpreadBps, curvePoints);

      if (result.clean_price && !result.error) {
        setPrice(result.clean_price);
      } else if (result.error) {
        setError(result.error);
      }
    } catch (err) {
      console.error('Spread solve error:', err);
      setError('Failed to calculate price from spread');
    }
  }, [wasmModule, bond, treasuryCurve]);

  // Handle G-spread change - calculate price from target G-spread
  const handleGSpreadChange = useCallback((targetGSpreadBps) => {
    if (!wasmModule) return;

    try {
      const bondParams = {
        coupon_rate: bond.couponRate,
        maturity_date: bond.maturityDate,
        issue_date: bond.issueDate,
        settlement_date: bond.settlementDate,
        face_value: bond.faceValue,
        frequency: bond.frequency,
        day_count: bond.dayCount,
        currency: bond.currency,
        first_coupon_date: bond.firstCouponDate || null,
        call_schedule: null,
      };

      const curvePoints = generateCurvePoints(treasuryCurve, bond.maturityDate);
      const result = wasmModule.price_from_g_spread(bondParams, targetGSpreadBps, curvePoints);

      if (result.clean_price && !result.error) {
        setPrice(result.clean_price);
      } else if (result.error) {
        setError(result.error);
      }
    } catch (err) {
      console.error('G-spread solve error:', err);
      setError('Failed to calculate price from G-spread');
    }
  }, [wasmModule, bond, treasuryCurve]);

  // Handle Benchmark spread change - calculate price from target benchmark spread
  const handleBenchmarkSpreadChange = useCallback((targetBenchmarkSpreadBps, benchmarkTenor) => {
    if (!wasmModule) return;

    try {
      const bondParams = {
        coupon_rate: bond.couponRate,
        maturity_date: bond.maturityDate,
        issue_date: bond.issueDate,
        settlement_date: bond.settlementDate,
        face_value: bond.faceValue,
        frequency: bond.frequency,
        day_count: bond.dayCount,
        currency: bond.currency,
        first_coupon_date: bond.firstCouponDate || null,
        call_schedule: null,
      };

      const curvePoints = generateCurvePoints(treasuryCurve, bond.maturityDate);
      const result = wasmModule.price_from_benchmark_spread(
        bondParams,
        targetBenchmarkSpreadBps,
        benchmarkTenor || '10Y',
        curvePoints
      );

      if (result.clean_price && !result.error) {
        setPrice(result.clean_price);
      } else if (result.error) {
        setError(result.error);
      }
    } catch (err) {
      console.error('Benchmark spread solve error:', err);
      setError('Failed to calculate price from benchmark spread');
    }
  }, [wasmModule, bond, treasuryCurve]);

  // Handle Treasury curve rate change
  const handleCurveChange = useCallback((tenor, rate) => {
    setTreasuryCurve(prev => ({
      ...prev,
      [tenor]: rate,
    }));
  }, []);

  // Fetch live rates for current currency
  const handleFetchRates = useCallback(async () => {
    setIsFetchingRates(true);
    setError(null);
    try {
      const rates = await fetchRatesForCurrency(bond.currency, curveType);
      setTreasuryCurve(prev => ({ ...prev, ...rates }));
      setRatesLastUpdated(new Date().toLocaleString());
    } catch (err) {
      setError(`Failed to fetch ${curveType} rates. Using default values.`);
      console.error(err);
    } finally {
      setIsFetchingRates(false);
    }
  }, [bond.currency, curveType]);

  // Handle curve type change
  const handleCurveTypeChange = useCallback((newCurveType) => {
    setCurveType(newCurveType);
  }, []);

  // Reset to defaults
  const handleReset = useCallback(() => {
    setBond(getDefaultBond());
    setPrice(100.0);
    setTreasuryCurve(DEFAULT_TREASURY_CURVE);
    setRatesLastUpdated(null);
    setAnalysis(null);
    setCashFlows([]);
    setError(null);
  }, []);

  // Export results
  const handleExport = useCallback(() => {
    if (!analysis) return;

    const exportData = {
      bond,
      price,
      treasuryCurve,
      analysis,
      cashFlows,
      exportDate: new Date().toISOString(),
    };

    const blob = new Blob([JSON.stringify(exportData, null, 2)], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `bond-analysis-${bond.name.replace(/\s+/g, '-')}.json`;
    a.click();
    URL.revokeObjectURL(url);
  }, [bond, price, treasuryCurve, analysis, cashFlows]);

  // Import from JSON
  const handleImport = useCallback((data) => {
    try {
      // Import bond parameters
      if (data.bond) {
        setBond(prev => ({
          ...prev,
          ...data.bond,
        }));
      }

      // Import price
      if (typeof data.price === 'number') {
        setPrice(data.price);
      }

      // Import Treasury curve
      if (data.treasuryCurve) {
        setTreasuryCurve(prev => ({
          ...prev,
          ...data.treasuryCurve,
        }));
      }

      // Clear previous analysis - user should recalculate
      setAnalysis(null);
      setCashFlows([]);
      setError(null);

      // Auto-calculate after import
      setTimeout(() => calculate(), 100);
    } catch (err) {
      setError('Failed to import data: ' + err.message);
    }
  }, [calculate]);

  return (
    <div className="app">
      <Header
        bondName={bond.name}
        onBondNameChange={(name) => handleBondChange('name', name)}
        settlementDate={bond.settlementDate}
        onSettlementChange={(date) => handleBondChange('settlementDate', date)}
        currency={bond.currency}
        onCurrencyChange={(curr) => handleBondChange('currency', curr)}
        onCalculate={calculate}
        onReset={handleReset}
        onExport={handleExport}
        onImport={handleImport}
        isCalculating={isCalculating}
      />

      {error && (
        <div className="error-banner">
          <span className="error-icon">!</span>
          <span>{error}</span>
        </div>
      )}

      <div className="main-content grid-layout">
        {/* Row 1: Bond Details | Yield Analysis | Invoice */}
        <div className="grid-row row-1">
          <div className="grid-cell">
            <BondInput
              bond={bond}
              onChange={handleBondChange}
            />
            <ConventionSettings
              conventions={conventions}
              onChange={handleConventionChange}
              onApplyDefaults={handleApplyDefaults}
              wasmModule={wasmModule}
              expanded={conventionsExpanded}
              onToggle={() => setConventionsExpanded(!conventionsExpanded)}
              compoundingLinked={compoundingLinked}
              onCompoundingLinkToggle={setCompoundingLinked}
            />
          </div>
          <div className="grid-cell">
            <YieldAnalysis
              analysis={analysis}
              price={price}
              onPriceChange={handlePriceChange}
              onYieldChange={handleYieldChange}
              settlementDate={bond.settlementDate}
            />
          </div>
          <div className="grid-cell">
            <Invoice
              analysis={analysis}
              price={price}
              faceAmount={faceAmount}
              onFaceAmountChange={setFaceAmount}
            />
          </div>
        </div>

        {/* Row 2: Call Schedule | Spread Analysis | Benchmark */}
        <div className="grid-row row-2">
          <div className="grid-cell">
            <CallSchedule
              callSchedule={bond.callSchedule}
              maturityDate={bond.maturityDate}
              volatility={bond.volatility}
              onChange={(schedule) => handleBondChange('callSchedule', schedule)}
              onVolatilityChange={(vol) => handleBondChange('volatility', vol)}
            />
          </div>
          <div className="grid-cell">
            <SpreadAnalysis
              analysis={analysis}
              onSpreadChange={handleSpreadChange}
              onGSpreadChange={handleGSpreadChange}
            />
          </div>
          <div className="grid-cell">
            <Benchmark
              analysis={analysis}
              treasuryCurve={treasuryCurve}
              maturityDate={bond.maturityDate}
              settlementDate={bond.settlementDate}
              onBenchmarkSpreadChange={handleBenchmarkSpreadChange}
            />
          </div>
        </div>

        {/* Row 3: Risk Metrics | Benchmark Curve + Price/Yield Chart | Cash Flows */}
        <div className="grid-row row-3">
          <div className="grid-cell">
            <RiskMetrics
              analysis={analysis}
              faceAmount={faceAmount}
            />
          </div>
          <div className="grid-cell curves-cell">
            <TreasuryCurve
              treasuryCurve={treasuryCurve}
              currency={bond.currency}
              curveType={curveType}
              availableCurveTypes={CURVE_TYPES_BY_CURRENCY[bond.currency] || ['UST']}
              curveTypeNames={CURVE_TYPE_NAMES}
              onCurveTypeChange={handleCurveTypeChange}
              onCurveChange={handleCurveChange}
              onFetchRates={handleFetchRates}
              isFetchingRates={isFetchingRates}
              ratesLastUpdated={ratesLastUpdated}
            />
            <PriceYieldChart
              data={priceYieldData}
              currentPrice={price}
              currentYield={analysis?.ytm}
            />
          </div>
          <div className="grid-cell">
            <CashFlowTable
              cashFlows={cashFlows}
              analysis={analysis}
            />
          </div>
        </div>
      </div>
    </div>
  );
}

export default App;
