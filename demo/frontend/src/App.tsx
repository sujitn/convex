import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import {
  TrendingUp,
  BarChart3,
  Zap,
  Shield,
  Calculator,
  LineChart,
  CheckCircle2,
  Github,
  ExternalLink,
} from 'lucide-react';
import { checkHealth } from './lib/api';
import { cn } from './lib/utils';
import YieldCurveDemo from './components/YieldCurveDemo';
import BondPricingDemo from './components/BondPricingDemo';
import PortfolioDemo from './components/PortfolioDemo';

type DemoSection = 'curves' | 'pricing' | 'portfolio' | null;

function App() {
  const [activeDemo, setActiveDemo] = useState<DemoSection>(null);

  const { data: health, isLoading: healthLoading } = useQuery({
    queryKey: ['health'],
    queryFn: checkHealth,
    retry: 1,
    refetchInterval: 30000,
  });

  const isApiConnected = health?.status === 'ok';

  return (
    <div className="min-h-screen">
      {/* Header */}
      <header className="bg-white border-b border-slate-200">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="flex justify-between items-center h-16">
            <div className="flex items-center space-x-3">
              <div className="w-8 h-8 bg-primary-700 rounded-lg flex items-center justify-center">
                <TrendingUp className="w-5 h-5 text-white" />
              </div>
              <span className="text-xl font-bold text-slate-900">Convex</span>
              <span className="text-sm text-slate-500 hidden sm:inline">Fixed Income Analytics</span>
            </div>

            <div className="flex items-center space-x-4">
              <div className="flex items-center space-x-2">
                <div
                  className={cn(
                    'w-2 h-2 rounded-full',
                    healthLoading ? 'bg-yellow-400 animate-pulse' :
                    isApiConnected ? 'bg-gain' : 'bg-loss'
                  )}
                />
                <span className="text-sm text-slate-600">
                  {healthLoading ? 'Connecting...' :
                   isApiConnected ? `API v${health?.version}` : 'Offline'}
                </span>
              </div>
              <a
                href="https://github.com/sujitn/convex"
                target="_blank"
                rel="noopener noreferrer"
                className="text-slate-600 hover:text-slate-900"
              >
                <Github className="w-5 h-5" />
              </a>
            </div>
          </div>
        </div>
      </header>

      {/* Hero Section */}
      <section className="bg-gradient-to-br from-primary-900 via-primary-800 to-primary-900 text-white py-20">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="text-center">
            <h1 className="text-4xl sm:text-5xl font-bold mb-6">
              High-Performance Fixed Income Analytics
            </h1>
            <p className="text-xl text-primary-200 max-w-3xl mx-auto mb-8">
              Production-grade bond pricing, yield curve construction, portfolio analytics,
              and risk metrics. Built in Rust for sub-microsecond calculations.
            </p>
            <div className="flex flex-wrap justify-center gap-4">
              <button
                onClick={() => setActiveDemo(activeDemo === 'curves' ? null : 'curves')}
                className={cn(
                  "btn flex items-center space-x-2",
                  activeDemo === 'curves'
                    ? "bg-white text-primary-900"
                    : "bg-primary-700 text-white hover:bg-primary-600"
                )}
              >
                <LineChart className="w-4 h-4" />
                <span>Yield Curves</span>
              </button>
              <button
                onClick={() => setActiveDemo(activeDemo === 'pricing' ? null : 'pricing')}
                className={cn(
                  "btn flex items-center space-x-2",
                  activeDemo === 'pricing'
                    ? "bg-white text-primary-900"
                    : "bg-primary-700 text-white hover:bg-primary-600"
                )}
              >
                <Calculator className="w-4 h-4" />
                <span>Bond Pricing</span>
              </button>
              <button
                onClick={() => setActiveDemo(activeDemo === 'portfolio' ? null : 'portfolio')}
                className={cn(
                  "btn flex items-center space-x-2",
                  activeDemo === 'portfolio'
                    ? "bg-white text-primary-900"
                    : "bg-primary-700 text-white hover:bg-primary-600"
                )}
              >
                <BarChart3 className="w-4 h-4" />
                <span>Portfolio Analytics</span>
              </button>
            </div>
          </div>
        </div>
      </section>

      {/* Demo Section */}
      {activeDemo && (
        <section className="py-12 bg-slate-100">
          <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
            <div className="flex justify-between items-center mb-6">
              <h2 className="text-2xl font-bold text-slate-900">
                {activeDemo === 'curves' && 'Yield Curve Analytics'}
                {activeDemo === 'pricing' && 'Bond Pricing'}
                {activeDemo === 'portfolio' && 'Portfolio Analytics'}
              </h2>
              <button
                onClick={() => setActiveDemo(null)}
                className="text-slate-600 hover:text-slate-900"
              >
                Close
              </button>
            </div>

            {!isApiConnected ? (
              <div className="card text-center py-12">
                <p className="text-slate-600 mb-4">
                  API server not connected. Running in demo mode with sample data.
                </p>
                <p className="text-sm text-slate-500">
                  Deploy the Convex server to Fly.io or run locally to enable live calculations.
                </p>
              </div>
            ) : (
              <>
                {activeDemo === 'curves' && <YieldCurveDemo />}
                {activeDemo === 'pricing' && <BondPricingDemo />}
                {activeDemo === 'portfolio' && <PortfolioDemo />}
              </>
            )}
          </div>
        </section>
      )}

      {/* Features Grid */}
      <section className="py-16 bg-white">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="text-center mb-12">
            <h2 className="text-3xl font-bold text-slate-900 mb-4">
              Institutional-Grade Analytics
            </h2>
            <p className="text-lg text-slate-600 max-w-2xl mx-auto">
              Production-grade calculations with enterprise performance
            </p>
          </div>

          <div className="grid md:grid-cols-2 lg:grid-cols-3 gap-8">
            <FeatureCard
              icon={<Calculator className="w-6 h-6" />}
              title="Bond Pricing"
              description="YTM, YTW, YTC, Z-spread, I-spread, G-spread, OAS, ASW. Support for fixed, callable, FRN, zero coupon, and inflation-linked bonds."
            />
            <FeatureCard
              icon={<LineChart className="w-6 h-6" />}
              title="Yield Curves"
              description="Bootstrapping from deposits, FRAs, and swaps. Support for OIS, government, and credit curves with multiple interpolation methods."
            />
            <FeatureCard
              icon={<BarChart3 className="w-6 h-6" />}
              title="Risk Analytics"
              description="Modified duration, Macaulay duration, effective duration, convexity, DV01, key rate durations, and spread duration."
            />
            <FeatureCard
              icon={<TrendingUp className="w-6 h-6" />}
              title="Portfolio Analytics"
              description="NAV calculation, duration contribution, sector/rating bucketing, benchmark comparison, and tracking error."
            />
            <FeatureCard
              icon={<Shield className="w-6 h-6" />}
              title="Stress Testing"
              description="Parallel and non-parallel rate shocks, spread widening scenarios, historical scenarios, and custom stress tests."
            />
            <FeatureCard
              icon={<Zap className="w-6 h-6" />}
              title="Real-Time Streaming"
              description="WebSocket-based quote streaming, batch pricing, and ETF iNAV calculations with sub-millisecond latency."
            />
          </div>
        </div>
      </section>

      {/* Bond Types Section */}
      <section className="py-16 bg-slate-50">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="text-center mb-12">
            <h2 className="text-3xl font-bold text-slate-900 mb-4">
              Comprehensive Bond Support
            </h2>
          </div>

          <div className="grid md:grid-cols-2 lg:grid-cols-4 gap-6">
            <BondTypeCard
              type="Fixed Rate"
              examples={['Corporate', 'Treasury', 'Agency']}
              metrics={['YTM', 'Duration', 'Z-spread']}
            />
            <BondTypeCard
              type="Callable"
              examples={['Corporate', 'Municipal']}
              metrics={['YTW', 'OAS', 'Effective Duration']}
            />
            <BondTypeCard
              type="Floating Rate"
              examples={['FRN', 'Treasury FRN']}
              metrics={['Discount Margin', 'Spread Duration']}
            />
            <BondTypeCard
              type="Zero Coupon"
              examples={['Treasury STRIPS', 'Corporate Zero']}
              metrics={['Spot Rate', 'Duration']}
            />
          </div>
        </div>
      </section>

      {/* Architecture Section */}
      <section className="py-16 bg-white">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="grid lg:grid-cols-2 gap-12 items-center">
            <div>
              <h2 className="text-3xl font-bold text-slate-900 mb-6">
                Built for Performance
              </h2>
              <p className="text-lg text-slate-600 mb-6">
                Core analytics written in Rust with multiple integration options:
              </p>
              <ul className="space-y-4">
                <ListItem>REST API with JSON responses</ListItem>
                <ListItem>WebSocket for real-time streaming</ListItem>
                <ListItem>Excel add-in via FFI</ListItem>
                <ListItem>WebAssembly for browser calculations</ListItem>
              </ul>
            </div>
            <div className="bg-slate-900 rounded-xl p-6 font-mono text-sm">
              <div className="text-slate-400 mb-4">// Example API Response</div>
              <pre className="text-green-400 overflow-x-auto">
{`{
  "instrument_id": "AAPL-5.0-2030",
  "clean_price_mid": 102.345,
  "ytm_mid": 0.0465,
  "z_spread_mid": 32.5,
  "modified_duration": 4.23,
  "convexity": 21.5,
  "dv01": 0.0423
}`}
              </pre>
            </div>
          </div>
        </div>
      </section>

      {/* CTA Section */}
      <section className="py-16 bg-gradient-to-r from-primary-800 to-primary-900 text-white">
        <div className="max-w-4xl mx-auto px-4 sm:px-6 lg:px-8 text-center">
          <h2 className="text-3xl font-bold mb-6">Ready to Get Started?</h2>
          <p className="text-xl text-primary-200 mb-8">
            Deploy your own Convex server or integrate the library into your application.
          </p>
          <div className="flex flex-wrap justify-center gap-4">
            <a
              href="https://github.com/sujitn/convex"
              target="_blank"
              rel="noopener noreferrer"
              className="btn bg-white text-primary-900 hover:bg-primary-50 flex items-center space-x-2"
            >
              <Github className="w-4 h-4" />
              <span>View on GitHub</span>
            </a>
            <a
              href="https://docs.rs/convex-bonds"
              target="_blank"
              rel="noopener noreferrer"
              className="btn border border-white/30 text-white hover:bg-white/10 flex items-center space-x-2"
            >
              <ExternalLink className="w-4 h-4" />
              <span>Documentation</span>
            </a>
          </div>
        </div>
      </section>

      {/* Footer */}
      <footer className="bg-slate-900 text-slate-400 py-8">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="flex flex-col md:flex-row justify-between items-center">
            <div className="flex items-center space-x-2 mb-4 md:mb-0">
              <TrendingUp className="w-5 h-5 text-primary-500" />
              <span className="font-semibold text-white">Convex</span>
              <span className="text-sm">v{health?.version || '0.10.32'}</span>
            </div>
            <div className="text-sm">
              MIT License | Built with Rust
            </div>
          </div>
        </div>
      </footer>
    </div>
  );
}

function FeatureCard({
  icon,
  title,
  description,
}: {
  icon: React.ReactNode;
  title: string;
  description: string;
}) {
  return (
    <div className="card hover:shadow-md transition-shadow">
      <div className="w-12 h-12 bg-primary-100 rounded-lg flex items-center justify-center text-primary-700 mb-4">
        {icon}
      </div>
      <h3 className="text-lg font-semibold text-slate-900 mb-2">{title}</h3>
      <p className="text-slate-600">{description}</p>
    </div>
  );
}

function BondTypeCard({
  type,
  examples,
  metrics,
}: {
  type: string;
  examples: string[];
  metrics: string[];
}) {
  return (
    <div className="card">
      <h3 className="text-lg font-semibold text-slate-900 mb-3">{type}</h3>
      <div className="mb-3">
        <div className="text-xs text-slate-500 uppercase mb-1">Examples</div>
        <div className="flex flex-wrap gap-1">
          {examples.map((ex) => (
            <span key={ex} className="badge badge-blue">{ex}</span>
          ))}
        </div>
      </div>
      <div>
        <div className="text-xs text-slate-500 uppercase mb-1">Key Metrics</div>
        <div className="text-sm text-slate-700">{metrics.join(' | ')}</div>
      </div>
    </div>
  );
}

function ListItem({ children }: { children: React.ReactNode }) {
  return (
    <li className="flex items-center space-x-3">
      <CheckCircle2 className="w-5 h-5 text-gain flex-shrink-0" />
      <span className="text-slate-700">{children}</span>
    </li>
  );
}

export default App;
