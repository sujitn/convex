// =============================================================================
// Hooks Index
// Export all custom hooks for the demo application
// =============================================================================

export {
  useWebSocket,
  type WebSocketMessage,
  type BondQuoteMessage,
  type EtfQuoteMessage,
  type PortfolioMessage,
  type ConnectionState,
} from './useWebSocket';

export {
  useBondQuoteStream,
  type StreamedQuote,
} from './useBondQuoteStream';

export {
  useEtfStream,
  type StreamedEtfQuote,
} from './useEtfStream';
