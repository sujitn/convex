// =============================================================================
// ETF Streaming Demo
// Full ETF analytics with live iNAV, creation baskets, and arbitrage monitoring
// Now with WebSocket streaming integration for live bond prices and iNAV
// =============================================================================

import { useState, useEffect, useCallback } from 'react';
import {
  TrendingUp,
  TrendingDown,
  BarChart3,
  RefreshCw,
  ChevronDown,
  ChevronUp,
  AlertTriangle,
  CheckCircle,
  XCircle,
  ArrowUpDown,
  Package,
  PackageMinus,
  Activity,
  Wifi,
  WifiOff,
  Radio,
  Zap,
  Search,
} from 'lucide-react';
import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
  Legend,
  AreaChart,
  Area,
} from 'recharts';
import {
  fetchETFHoldingsFromProvider,
  fetchNAVHistory,
  fetchCreationBasket,
  type DataProviderETFResponse,
} from '../lib/api';
import { useEtfStream } from '../hooks/useEtfStream';
import { useBondQuoteStream } from '../hooks/useBondQuoteStream';

// ETF options
const ETF_OPTIONS = ['LQD', 'HYG', 'TLT', 'AGG', 'BND'];

// Data source mode
type DataSourceMode = 'simulated' | 'websocket';

export function ETFStreamingDemo() {
  const [selectedETF, setSelectedETF] = useState('LQD');
  const [etfData, setEtfData] = useState<DataProviderETFResponse | null>(null);
  const [dataSource, setDataSource] = useState<DataSourceMode>('simulated');
  const [holdingsSearch, setHoldingsSearch] = useState('');
  const [holdingsExpanded, setHoldingsExpanded] = useState(false);

  // WebSocket streams for live data
  const {
    connectionState: etfConnectionState,
    latency: etfLatency,
    tickCount: etfTickCount,
    start: startEtfStream,
    stop: stopEtfStream,
    subscribe: subscribeEtf,
  } = useEtfStream({ autoConnect: false });

  const {
    connectionState: bondConnectionState,
    tickCount: bondTickCount,
    start: startBondStream,
    stop: stopBondStream,
    getQuote: getBondQuote,
  } = useBondQuoteStream({ autoConnect: false, subscribeAll: true });
  const [navHistory, setNavHistory] = useState<Array<{
    date: string;
    nav: number;
    market_price: number;
    premium_discount: number;
  }> | null>(null);
  const [basketData, setBasketData] = useState<{
    etf_ticker: string;
    creation_unit_size: number;
    cash_component: number;
    total_value: number;
    nav_per_share: number;
    components: Array<{
      cusip: string;
      name: string;
      shares: number;
      weight: number;
      market_value: number;
    }>;
    estimated_expenses: number;
  } | null>(null);
  const [arbData, setArbData] = useState<{
    nav_per_share: number;
    market_price: number;
    arbitrage: {
      premium_discount_pct: number;
      premium_discount_bps: number;
      action: 'create' | 'redeem' | 'none';
      gross_profit: number;
      net_profit: number;
      profitable: boolean;
    };
  } | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [basketExpanded, setBasketExpanded] = useState(false);
  const [basketType, setBasketType] = useState<'creation' | 'redemption'>('creation');

  // Simulate live market price movement
  const [liveMarketPrice, setLiveMarketPrice] = useState<number | null>(null);
  const [priceHistory, setPriceHistory] = useState<Array<{ time: string; nav: number; price: number }>>([]);

  // Load ETF data
  useEffect(() => {
    loadETFData();
  }, [selectedETF]);

  // Simulate live price updates
  useEffect(() => {
    if (!etfData) return;

    const basePrice = etfData.etf.nav;
    setLiveMarketPrice(basePrice);

    const interval = setInterval(() => {
      setLiveMarketPrice((prev) => {
        if (!prev) return basePrice;
        // Small random walk
        const change = (Math.random() - 0.5) * 0.04;
        return Math.round((prev + change) * 100) / 100;
      });
    }, 2000);

    return () => clearInterval(interval);
  }, [etfData]);

  // Update price history for chart
  useEffect(() => {
    if (!etfData || !liveMarketPrice) return;

    setPriceHistory((prev) => {
      const now = new Date();
      const timeStr = now.toLocaleTimeString('en-US', { hour12: false, hour: '2-digit', minute: '2-digit', second: '2-digit' });
      const newPoint = { time: timeStr, nav: etfData.etf.nav, price: liveMarketPrice };
      const updated = [...prev, newPoint];
      if (updated.length > 30) updated.shift();
      return updated;
    });
  }, [liveMarketPrice, etfData]);

  // Update arbitrage with live price
  useEffect(() => {
    if (!liveMarketPrice || !etfData) return;

    // Calculate arbitrage locally for live updates
    const nav = etfData.etf.nav;
    const premiumDiscount = ((liveMarketPrice - nav) / nav) * 100;
    const premiumBps = premiumDiscount * 100;
    const creationUnitSize = basketData?.creation_unit_size || 50000;
    const creationFee = basketData?.estimated_expenses || 500;
    const grossProfit = (liveMarketPrice - nav) * creationUnitSize;
    const netProfit = Math.abs(grossProfit) - creationFee;

    setArbData({
      nav_per_share: nav,
      market_price: liveMarketPrice,
      arbitrage: {
        premium_discount_pct: Math.round(premiumDiscount * 100) / 100,
        premium_discount_bps: Math.round(premiumBps),
        action: premiumBps > 15 ? 'create' : premiumBps < -15 ? 'redeem' : 'none',
        gross_profit: Math.round(grossProfit * 100) / 100,
        net_profit: Math.round(netProfit * 100) / 100,
        profitable: netProfit > 0 && Math.abs(premiumBps) > 15,
      },
    });
  }, [liveMarketPrice, etfData, basketData]);

  const loadETFData = async () => {
    setIsLoading(true);
    setError(null);
    try {
      const [holdings, history, basket] = await Promise.all([
        fetchETFHoldingsFromProvider(selectedETF),
        fetchNAVHistory(selectedETF, 30),
        fetchCreationBasket(selectedETF),
      ]);

      setEtfData(holdings);
      setNavHistory(history.history);
      setBasketData(basket.basket);
      setPriceHistory([]);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to load ETF data');
    } finally {
      setIsLoading(false);
    }
  };

  const handleRefresh = () => {
    loadETFData();
  };

  // Start WebSocket streaming
  const handleStartWebSocket = useCallback(() => {
    setDataSource('websocket');
    startEtfStream();
    startBondStream();
    // Subscribe to the selected ETF (bond subscription is automatic with subscribeAll: true)
    setTimeout(() => {
      subscribeEtf([selectedETF]);
    }, 500);
  }, [startEtfStream, startBondStream, subscribeEtf, selectedETF]);

  // Stop WebSocket streaming
  const handleStopWebSocket = useCallback(() => {
    stopEtfStream();
    stopBondStream();
    setDataSource('simulated');
  }, [stopEtfStream, stopBondStream]);

  // Get filtered holdings with live prices
  const getFilteredHoldings = useCallback(() => {
    if (!etfData) return [];

    return etfData.holdings
      .filter((h) => {
        if (!holdingsSearch) return true;
        const search = holdingsSearch.toLowerCase();
        return (
          h.cusip.toLowerCase().includes(search) ||
          h.issuer.toLowerCase().includes(search) ||
          h.description.toLowerCase().includes(search) ||
          h.sector?.toLowerCase().includes(search)
        );
      })
      .map((holding) => {
        // Get live price from WebSocket if available
        const liveQuote = getBondQuote(holding.cusip);
        return {
          ...holding,
          livePrice: liveQuote?.mid,
          liveYield: liveQuote?.ytm,
          liveDuration: liveQuote?.duration,
          priceChange: liveQuote?.priceChange,
          hasLiveData: !!liveQuote,
        };
      });
  }, [etfData, holdingsSearch, getBondQuote]);

  // Connection status helper
  const isWebSocketConnected = etfConnectionState === 'connected' || bondConnectionState === 'connected';

  if (isLoading && !etfData) {
    return (
      <div className="p-6 text-center">
        <RefreshCw className="h-8 w-8 animate-spin mx-auto text-gray-400" />
        <p className="mt-2 text-gray-600">Loading ETF data...</p>
      </div>
    );
  }

  const premiumDiscount = arbData?.arbitrage.premium_discount_pct || 0;
  const isPremium = premiumDiscount > 0;

  return (
    <div className="p-6 space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-2xl font-bold text-gray-900">ETF Streaming Analytics</h2>
          <p className="text-gray-600">Live iNAV tracking, creation baskets, and arbitrage monitoring</p>
        </div>
        <div className="flex items-center gap-4">
          <select
            value={selectedETF}
            onChange={(e) => setSelectedETF(e.target.value)}
            className="border rounded-lg px-3 py-2"
          >
            {ETF_OPTIONS.map((etf) => (
              <option key={etf} value={etf}>{etf}</option>
            ))}
          </select>
          <button
            onClick={handleRefresh}
            disabled={isLoading}
            className="flex items-center gap-2 px-4 py-2 bg-gray-100 hover:bg-gray-200 rounded-lg disabled:opacity-50"
          >
            <RefreshCw className={`h-4 w-4 ${isLoading ? 'animate-spin' : ''}`} />
            Refresh
          </button>
        </div>
      </div>

      {/* WebSocket Connection Panel */}
      <div className="bg-white rounded-lg shadow-sm border p-4">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-4">
            <div className="flex items-center gap-2">
              {isWebSocketConnected ? (
                <>
                  <Wifi className="h-5 w-5 text-green-500" />
                  <span className="text-green-600 font-medium">WebSocket Connected</span>
                </>
              ) : (
                <>
                  <WifiOff className="h-5 w-5 text-gray-400" />
                  <span className="text-gray-500">WebSocket Disconnected</span>
                </>
              )}
            </div>
            {isWebSocketConnected && (
              <>
                <div className="border-l pl-4 flex items-center gap-2">
                  <Radio className="h-4 w-4 text-blue-500" />
                  <span className="text-sm text-gray-600">
                    ETF: {etfTickCount} ticks | Bonds: {bondTickCount} ticks
                  </span>
                </div>
                {etfLatency && (
                  <div className="border-l pl-4 flex items-center gap-2">
                    <Zap className="h-4 w-4 text-yellow-500" />
                    <span className="text-sm text-gray-600">{etfLatency}ms latency</span>
                  </div>
                )}
              </>
            )}
          </div>
          <div className="flex items-center gap-2">
            <span className="text-sm text-gray-600">Data Source:</span>
            <span className={`text-sm font-medium ${dataSource === 'websocket' ? 'text-green-600' : 'text-gray-600'}`}>
              {dataSource === 'websocket' ? 'Live WebSocket' : 'Simulated'}
            </span>
            {!isWebSocketConnected ? (
              <button
                onClick={handleStartWebSocket}
                className="flex items-center gap-2 px-4 py-2 bg-green-600 text-white hover:bg-green-700 rounded-lg ml-4"
              >
                <Wifi className="h-4 w-4" />
                Connect WebSocket
              </button>
            ) : (
              <button
                onClick={handleStopWebSocket}
                className="flex items-center gap-2 px-4 py-2 bg-red-600 text-white hover:bg-red-700 rounded-lg ml-4"
              >
                <WifiOff className="h-4 w-4" />
                Disconnect
              </button>
            )}
          </div>
        </div>
      </div>

      {error && (
        <div className="bg-red-50 border border-red-200 text-red-700 px-4 py-3 rounded-lg flex items-center gap-2">
          <AlertTriangle className="h-5 w-5" />
          {error}
        </div>
      )}

      {etfData && (
        <>
          {/* NAV/iNAV Panel */}
          <div className="bg-white rounded-lg shadow-sm border p-6">
            <div className="flex items-start justify-between mb-6">
              <div>
                <h3 className="text-lg font-semibold text-gray-900">{etfData.etf.ticker}</h3>
                <p className="text-sm text-gray-600">{etfData.etf.name}</p>
              </div>
              <div className="flex items-center gap-2">
                <Activity className="h-4 w-4 text-green-500 animate-pulse" />
                <span className="text-sm text-green-600">Live</span>
              </div>
            </div>

            <div className="grid grid-cols-4 gap-6 mb-6">
              <div>
                <div className="text-sm text-gray-600">NAV</div>
                <div className="text-2xl font-bold text-gray-900">${etfData.etf.nav.toFixed(2)}</div>
              </div>
              <div>
                <div className="text-sm text-gray-600">Market Price (Live)</div>
                <div className="text-2xl font-bold text-gray-900">
                  ${liveMarketPrice?.toFixed(2) || '-'}
                </div>
              </div>
              <div>
                <div className="text-sm text-gray-600">Premium/Discount</div>
                <div className={`text-2xl font-bold ${isPremium ? 'text-green-600' : 'text-red-600'}`}>
                  {isPremium ? '+' : ''}{premiumDiscount.toFixed(2)}%
                  {isPremium ? <TrendingUp className="inline ml-1 h-5 w-5" /> : <TrendingDown className="inline ml-1 h-5 w-5" />}
                </div>
              </div>
              <div>
                <div className="text-sm text-gray-600">Holdings</div>
                <div className="text-2xl font-bold text-gray-900">{etfData.etf.holdings_count}</div>
              </div>
            </div>

            {/* Live Price Chart */}
            {priceHistory.length > 1 && (
              <div className="h-48">
                <ResponsiveContainer width="100%" height="100%">
                  <LineChart data={priceHistory}>
                    <CartesianGrid strokeDasharray="3 3" stroke="#e5e7eb" />
                    <XAxis dataKey="time" tick={{ fontSize: 10 }} />
                    <YAxis domain={['auto', 'auto']} tick={{ fontSize: 10 }} />
                    <Tooltip />
                    <Legend />
                    <Line type="monotone" dataKey="nav" name="NAV" stroke="#3b82f6" strokeWidth={2} dot={false} />
                    <Line type="monotone" dataKey="price" name="Market Price" stroke="#10b981" strokeWidth={2} dot={false} />
                  </LineChart>
                </ResponsiveContainer>
              </div>
            )}
          </div>

          {/* Arbitrage Monitor */}
          <div className="bg-white rounded-lg shadow-sm border p-6">
            <h3 className="text-lg font-semibold mb-4 flex items-center gap-2">
              <ArrowUpDown className="h-5 w-5" />
              Arbitrage Monitor
            </h3>

            {arbData && (
              <div className="grid grid-cols-2 gap-6">
                <div className="space-y-4">
                  <div className="flex justify-between items-center">
                    <span className="text-gray-600">Premium/Discount</span>
                    <span className={`font-bold ${arbData.arbitrage.premium_discount_pct >= 0 ? 'text-green-600' : 'text-red-600'}`}>
                      {arbData.arbitrage.premium_discount_pct >= 0 ? '+' : ''}{arbData.arbitrage.premium_discount_pct.toFixed(2)}%
                      ({arbData.arbitrage.premium_discount_bps} bps)
                    </span>
                  </div>
                  <div className="flex justify-between items-center">
                    <span className="text-gray-600">Recommended Action</span>
                    <span className={`font-bold ${
                      arbData.arbitrage.action === 'create' ? 'text-green-600' :
                      arbData.arbitrage.action === 'redeem' ? 'text-blue-600' : 'text-gray-500'
                    }`}>
                      {arbData.arbitrage.action === 'create' ? 'CREATE & SELL' :
                       arbData.arbitrage.action === 'redeem' ? 'BUY & REDEEM' : 'NO ACTION'}
                    </span>
                  </div>
                  <div className="flex justify-between items-center">
                    <span className="text-gray-600">Threshold</span>
                    <span className="text-gray-900">15 bps</span>
                  </div>
                </div>

                <div className="space-y-4">
                  <div className="flex justify-between items-center">
                    <span className="text-gray-600">Gross Profit (per CU)</span>
                    <span className={`font-bold ${arbData.arbitrage.gross_profit >= 0 ? 'text-green-600' : 'text-red-600'}`}>
                      ${Math.abs(arbData.arbitrage.gross_profit).toLocaleString()}
                    </span>
                  </div>
                  <div className="flex justify-between items-center">
                    <span className="text-gray-600">Creation Fee</span>
                    <span className="text-gray-900">${basketData?.estimated_expenses || 500}</span>
                  </div>
                  <div className="flex justify-between items-center">
                    <span className="text-gray-600">Net Profit</span>
                    <span className={`font-bold flex items-center gap-1 ${arbData.arbitrage.profitable ? 'text-green-600' : 'text-red-600'}`}>
                      ${Math.abs(arbData.arbitrage.net_profit).toLocaleString()}
                      {arbData.arbitrage.profitable ? <CheckCircle className="h-4 w-4" /> : <XCircle className="h-4 w-4" />}
                    </span>
                  </div>
                </div>
              </div>
            )}

            {arbData?.arbitrage.profitable && (
              <div className="mt-4 p-3 bg-green-50 border border-green-200 rounded-lg">
                <div className="flex items-center gap-2 text-green-700">
                  <CheckCircle className="h-5 w-5" />
                  <span className="font-medium">
                    Arbitrage opportunity available: {arbData.arbitrage.action === 'create' ? 'Create' : 'Redeem'} creation units for ${arbData.arbitrage.net_profit.toLocaleString()} net profit
                  </span>
                </div>
              </div>
            )}
          </div>

          {/* Creation/Redemption Basket */}
          {basketData && (
            <div className="bg-white rounded-lg shadow-sm border overflow-hidden">
              <button
                className="w-full px-6 py-4 flex items-center justify-between bg-gray-50 hover:bg-gray-100"
                onClick={() => setBasketExpanded(!basketExpanded)}
              >
                <h3 className="text-lg font-semibold flex items-center gap-2">
                  {basketType === 'creation' ? <Package className="h-5 w-5" /> : <PackageMinus className="h-5 w-5" />}
                  {basketType === 'creation' ? 'Creation' : 'Redemption'} Basket ({basketData.creation_unit_size.toLocaleString()} shares)
                </h3>
                {basketExpanded ? <ChevronUp className="h-5 w-5" /> : <ChevronDown className="h-5 w-5" />}
              </button>

              {basketExpanded && (
                <div className="p-6">
                  {/* Creation/Redemption Toggle */}
                  <div className="flex gap-2 mb-6">
                    <button
                      className={`flex items-center gap-2 px-4 py-2 rounded-lg font-medium transition-colors ${
                        basketType === 'creation'
                          ? 'bg-green-100 text-green-700 border border-green-300'
                          : 'bg-gray-100 text-gray-600 hover:bg-gray-200'
                      }`}
                      onClick={() => setBasketType('creation')}
                    >
                      <Package className="h-4 w-4" />
                      Creation
                    </button>
                    <button
                      className={`flex items-center gap-2 px-4 py-2 rounded-lg font-medium transition-colors ${
                        basketType === 'redemption'
                          ? 'bg-blue-100 text-blue-700 border border-blue-300'
                          : 'bg-gray-100 text-gray-600 hover:bg-gray-200'
                      }`}
                      onClick={() => setBasketType('redemption')}
                    >
                      <PackageMinus className="h-4 w-4" />
                      Redemption
                    </button>
                  </div>

                  {/* Process Description */}
                  <div className={`p-4 rounded-lg mb-6 ${basketType === 'creation' ? 'bg-green-50 border border-green-200' : 'bg-blue-50 border border-blue-200'}`}>
                    {basketType === 'creation' ? (
                      <div className="text-sm text-green-800">
                        <strong>Creation Process:</strong> Authorized Participant delivers securities + cash to fund custodian → Receives {basketData.creation_unit_size.toLocaleString()} ETF shares
                      </div>
                    ) : (
                      <div className="text-sm text-blue-800">
                        <strong>Redemption Process:</strong> Authorized Participant delivers {basketData.creation_unit_size.toLocaleString()} ETF shares to fund → Receives securities + cash from custodian
                      </div>
                    )}
                  </div>

                  <div className="grid grid-cols-4 gap-4 mb-6">
                    <div>
                      <div className="text-sm text-gray-600">Securities {basketType === 'creation' ? 'Delivered' : 'Received'}</div>
                      <div className="text-lg font-bold text-gray-900">
                        ${(basketData.total_value - basketData.cash_component).toLocaleString()}
                      </div>
                    </div>
                    <div>
                      <div className="text-sm text-gray-600">Cash {basketType === 'creation' ? 'Delivered' : 'Received'}</div>
                      <div className="text-lg font-bold text-gray-900">
                        ${basketData.cash_component.toLocaleString()}
                      </div>
                    </div>
                    <div>
                      <div className="text-sm text-gray-600">Total Value</div>
                      <div className="text-lg font-bold text-gray-900">
                        ${basketData.total_value.toLocaleString()}
                      </div>
                    </div>
                    <div>
                      <div className="text-sm text-gray-600">NAV per Share</div>
                      <div className="text-lg font-bold text-gray-900">
                        ${basketData.nav_per_share.toFixed(2)}
                      </div>
                    </div>
                  </div>

                  {/* ETF Shares */}
                  <div className={`p-4 rounded-lg mb-6 ${basketType === 'creation' ? 'bg-green-100' : 'bg-blue-100'}`}>
                    <div className="flex items-center justify-between">
                      <span className={`font-medium ${basketType === 'creation' ? 'text-green-800' : 'text-blue-800'}`}>
                        ETF Shares {basketType === 'creation' ? 'Received' : 'Delivered'}
                      </span>
                      <span className={`text-xl font-bold ${basketType === 'creation' ? 'text-green-900' : 'text-blue-900'}`}>
                        {basketData.creation_unit_size.toLocaleString()} shares
                      </span>
                    </div>
                  </div>

                  <div className="max-h-64 overflow-y-auto">
                    <table className="w-full">
                      <thead className="bg-gray-50 sticky top-0">
                        <tr>
                          <th className="px-4 py-2 text-left text-xs font-medium text-gray-500 uppercase">CUSIP</th>
                          <th className="px-4 py-2 text-left text-xs font-medium text-gray-500 uppercase">Name</th>
                          <th className="px-4 py-2 text-right text-xs font-medium text-gray-500 uppercase">Shares</th>
                          <th className="px-4 py-2 text-right text-xs font-medium text-gray-500 uppercase">Weight</th>
                          <th className="px-4 py-2 text-right text-xs font-medium text-gray-500 uppercase">Value</th>
                        </tr>
                      </thead>
                      <tbody>
                        {basketData.components.slice(0, 20).map((component, index) => (
                          <tr key={index} className="border-t">
                            <td className="px-4 py-2 font-mono text-sm">{component.cusip}</td>
                            <td className="px-4 py-2 text-sm truncate max-w-xs">{component.name}</td>
                            <td className="px-4 py-2 text-right text-sm">{component.shares.toLocaleString()}</td>
                            <td className="px-4 py-2 text-right text-sm">{component.weight.toFixed(2)}%</td>
                            <td className="px-4 py-2 text-right text-sm">${component.market_value.toLocaleString()}</td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                    {basketData.components.length > 20 && (
                      <div className="text-center py-2 text-sm text-gray-500">
                        Showing 20 of {basketData.components.length} components
                      </div>
                    )}
                  </div>
                </div>
              )}
            </div>
          )}

          {/* NAV History Chart */}
          {navHistory && navHistory.length > 0 && (
            <div className="bg-white rounded-lg shadow-sm border p-6">
              <h3 className="text-lg font-semibold mb-4 flex items-center gap-2">
                <BarChart3 className="h-5 w-5" />
                30-Day NAV History
              </h3>

              <div className="h-64">
                <ResponsiveContainer width="100%" height="100%">
                  <AreaChart data={navHistory}>
                    <CartesianGrid strokeDasharray="3 3" stroke="#e5e7eb" />
                    <XAxis
                      dataKey="date"
                      tick={{ fontSize: 10 }}
                      tickFormatter={(val) => new Date(val).toLocaleDateString('en-US', { month: 'short', day: 'numeric' })}
                    />
                    <YAxis domain={['auto', 'auto']} tick={{ fontSize: 10 }} />
                    <Tooltip
                      labelFormatter={(val) => new Date(val).toLocaleDateString()}
                      formatter={(val: number, name: string) => [
                        name === 'premium_discount' ? `${val.toFixed(2)}%` : `$${val.toFixed(2)}`,
                        name === 'nav' ? 'NAV' : name === 'market_price' ? 'Price' : 'Premium/Disc'
                      ]}
                    />
                    <Legend />
                    <Area type="monotone" dataKey="nav" name="NAV" stroke="#3b82f6" fill="#3b82f6" fillOpacity={0.1} />
                    <Area type="monotone" dataKey="market_price" name="Price" stroke="#10b981" fill="#10b981" fillOpacity={0.1} />
                  </AreaChart>
                </ResponsiveContainer>
              </div>
            </div>
          )}

          {/* Key Metrics */}
          <div className="grid grid-cols-4 gap-4">
            <div className="bg-white rounded-lg shadow-sm border p-4">
              <div className="text-sm text-gray-600">Duration</div>
              <div className="text-xl font-bold text-gray-900">{etfData.metrics.weighted_duration.toFixed(2)}</div>
            </div>
            <div className="bg-white rounded-lg shadow-sm border p-4">
              <div className="text-sm text-gray-600">Yield</div>
              <div className="text-xl font-bold text-gray-900">{(etfData.metrics.weighted_yield * 100).toFixed(2)}%</div>
            </div>
            <div className="bg-white rounded-lg shadow-sm border p-4">
              <div className="text-sm text-gray-600">Avg Coupon</div>
              <div className="text-xl font-bold text-gray-900">{etfData.metrics.weighted_coupon.toFixed(2)}%</div>
            </div>
            <div className="bg-white rounded-lg shadow-sm border p-4">
              <div className="text-sm text-gray-600">AUM</div>
              <div className="text-xl font-bold text-gray-900">${(etfData.etf.aum / 1e9).toFixed(1)}B</div>
            </div>
          </div>

          {/* Holdings Grid with Live Prices */}
          <div className="bg-white rounded-lg shadow-sm border overflow-hidden">
            <button
              className="w-full px-6 py-4 flex items-center justify-between bg-gray-50 hover:bg-gray-100"
              onClick={() => setHoldingsExpanded(!holdingsExpanded)}
            >
              <h3 className="text-lg font-semibold flex items-center gap-2">
                <BarChart3 className="h-5 w-5" />
                Holdings ({etfData.holdings.length})
                {isWebSocketConnected && (
                  <span className="text-xs bg-green-100 text-green-700 px-2 py-1 rounded-full ml-2">
                    Live Prices
                  </span>
                )}
              </h3>
              <div className="flex items-center gap-4">
                {holdingsExpanded && (
                  <div className="relative" onClick={(e) => e.stopPropagation()}>
                    <Search className="absolute left-3 top-1/2 transform -translate-y-1/2 h-4 w-4 text-gray-400" />
                    <input
                      type="text"
                      placeholder="Search holdings..."
                      value={holdingsSearch}
                      onChange={(e) => setHoldingsSearch(e.target.value)}
                      className="pl-9 pr-3 py-1 border rounded-lg text-sm w-48"
                    />
                  </div>
                )}
                {holdingsExpanded ? <ChevronUp className="h-5 w-5" /> : <ChevronDown className="h-5 w-5" />}
              </div>
            </button>

            {holdingsExpanded && (
              <div className="p-6">
                <div className="max-h-96 overflow-y-auto">
                  <table className="w-full">
                    <thead className="bg-gray-50 sticky top-0">
                      <tr>
                        <th className="px-4 py-2 text-left text-xs font-medium text-gray-500 uppercase">CUSIP</th>
                        <th className="px-4 py-2 text-left text-xs font-medium text-gray-500 uppercase">Issuer</th>
                        <th className="px-4 py-2 text-right text-xs font-medium text-gray-500 uppercase">Weight</th>
                        <th className="px-4 py-2 text-right text-xs font-medium text-gray-500 uppercase">Price</th>
                        <th className="px-4 py-2 text-right text-xs font-medium text-gray-500 uppercase">YTM</th>
                        <th className="px-4 py-2 text-right text-xs font-medium text-gray-500 uppercase">Duration</th>
                        <th className="px-4 py-2 text-center text-xs font-medium text-gray-500 uppercase">Status</th>
                      </tr>
                    </thead>
                    <tbody>
                      {getFilteredHoldings().slice(0, 50).map((holding, index) => (
                        <tr key={index} className={`border-t ${holding.hasLiveData ? 'bg-green-50' : ''}`}>
                          <td className="px-4 py-2 font-mono text-sm">{holding.cusip}</td>
                          <td className="px-4 py-2 text-sm truncate max-w-xs">{holding.issuer}</td>
                          <td className="px-4 py-2 text-right text-sm">{holding.weight.toFixed(2)}%</td>
                          <td className="px-4 py-2 text-right text-sm">
                            {holding.hasLiveData ? (
                              <span className="flex items-center justify-end gap-1">
                                ${holding.livePrice?.toFixed(2)}
                                {holding.priceChange !== undefined && holding.priceChange !== 0 && (
                                  <span className={holding.priceChange > 0 ? 'text-green-600' : 'text-red-600'}>
                                    {holding.priceChange > 0 ? <TrendingUp className="h-3 w-3" /> : <TrendingDown className="h-3 w-3" />}
                                  </span>
                                )}
                              </span>
                            ) : (
                              <span className="text-gray-400">${holding.market_value ? (holding.market_value / 100).toFixed(2) : '-'}</span>
                            )}
                          </td>
                          <td className="px-4 py-2 text-right text-sm">
                            {holding.hasLiveData && holding.liveYield ? (
                              <span className="text-green-700">{(holding.liveYield * 100).toFixed(2)}%</span>
                            ) : (
                              <span className="text-gray-400">-</span>
                            )}
                          </td>
                          <td className="px-4 py-2 text-right text-sm">
                            {holding.hasLiveData && holding.liveDuration ? (
                              <span className="text-green-700">{holding.liveDuration.toFixed(2)}</span>
                            ) : (
                              <span className="text-gray-400">-</span>
                            )}
                          </td>
                          <td className="px-4 py-2 text-center">
                            {holding.hasLiveData ? (
                              <span className="inline-flex items-center gap-1 text-green-600">
                                <Activity className="h-3 w-3 animate-pulse" />
                                <span className="text-xs">Live</span>
                              </span>
                            ) : (
                              <span className="text-xs text-gray-400">Static</span>
                            )}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                  {getFilteredHoldings().length > 50 && (
                    <div className="text-center py-2 text-sm text-gray-500">
                      Showing 50 of {getFilteredHoldings().length} holdings
                      {holdingsSearch && ` matching "${holdingsSearch}"`}
                    </div>
                  )}
                  {getFilteredHoldings().length === 0 && holdingsSearch && (
                    <div className="text-center py-4 text-gray-500">
                      No holdings match "{holdingsSearch}"
                    </div>
                  )}
                </div>

                {/* Live Data Summary */}
                {isWebSocketConnected && (
                  <div className="mt-4 pt-4 border-t">
                    <div className="flex items-center justify-between text-sm">
                      <span className="text-gray-600">
                        Live coverage: {getFilteredHoldings().filter(h => h.hasLiveData).length} of {getFilteredHoldings().length} holdings
                      </span>
                      <span className="text-gray-600">
                        Coverage: {((getFilteredHoldings().filter(h => h.hasLiveData).length / getFilteredHoldings().length) * 100).toFixed(1)}%
                      </span>
                    </div>
                  </div>
                )}
              </div>
            )}
          </div>
        </>
      )}
    </div>
  );
}

export default ETFStreamingDemo;
