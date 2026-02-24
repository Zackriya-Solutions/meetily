/**
 * Centralized Configuration for Pnyx
 * 
 * Automatically switches between development and production URLs
 * based on NODE_ENV environment variable.
 * 
 * Development (npm run dev): Uses localhost
 * Production (npm run build / Vercel): Uses env vars or meet.digest.lat
 */

const isDevelopment = process.env.NODE_ENV === 'development';

export const config = {
  // HTTP API base URL
  apiUrl: isDevelopment 
    ? 'http://localhost:5167'
    : (process.env.NEXT_PUBLIC_BACKEND_URL || 'https://meet.pnyx.com'),
  
  // WebSocket URL for real-time streaming
  wsUrl: isDevelopment
    ? 'ws://localhost:5167/ws/streaming-audio'
    : (process.env.NEXT_PUBLIC_WS_URL || 'wss://meet.pnyx.com/ws/streaming-audio'),
  
  // Debug mode - enables extra logging
  debug: isDevelopment,
  
  // Environment name for logging
  environment: isDevelopment ? 'development' : 'production',
};

// Log configuration on startup (client-side only)
if (typeof window !== 'undefined' && config.debug) {
  console.log('[Config] Environment:', config.environment);
  console.log('[Config] API URL:', config.apiUrl);
  console.log('[Config] WS URL:', config.wsUrl);
}

// Export individual values for convenience
export const { apiUrl, wsUrl, debug } = config;