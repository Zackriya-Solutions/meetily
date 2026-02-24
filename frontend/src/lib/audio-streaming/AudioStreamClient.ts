/**
 * Audio Stream Client for Real-Time Transcription
 *
 * Manages: Microphone → AudioWorklet → WebSocket → Backend
 *
 * Features:
 * - Continuous PCM audio streaming
 * - WebSocket connection management
 * - Partial and final transcript handling
 */

export interface StreamingCallbacks {
  onPartial?: (text: string, confidence: number, isStable: boolean) => void;
  onFinal?: (text: string, confidence: number, reason: string, timing?: { start: number, end: number, duration: number }) => void;
  onError?: (error: Error, code?: string) => void;
  onConnected?: (sessionId: string) => void;
  onDisconnected?: () => void;
}

import { wsUrl } from '../config';

export class AudioStreamClient {
  private audioContext: AudioContext | null = null;
  private audioWorklet: AudioWorkletNode | null = null;
  private websocket: WebSocket | null = null;
  private mediaStream: MediaStream | null = null;
  private callbacks: StreamingCallbacks = {};
  private isStreaming: boolean = false;

  // Robustness state
  private reconnectAttempts: number = 0;
  private maxReconnectAttempts: number = 5;
  private audioQueue: ArrayBuffer[] = []; // Changed to ArrayBuffer for combined data
  private isReconnecting: boolean = false;
  private intentionalClose: boolean = false;
  private sessionId: string | null = null;
  private meetingId: string | null = null;
  private authToken: string | null = null;
  private recordingStartTime: number = 0; // AudioContext.currentTime at recording start
  private heartbeatInterval: NodeJS.Timeout | null = null;
  private reconnectInFlight: Promise<void> | null = null;
  private maxBufferedChunks: number = 500;
  private pendingStopResolve: (() => void) | null = null;
  private pendingStopReject: ((reason?: unknown) => void) | null = null;

  constructor(
    private wsUrlOverride: string = wsUrl
  ) {}

  /**
   * Start streaming audio to backend
   */
  async start(
    callbacks: StreamingCallbacks,
    sessionId?: string,
    meetingId?: string,
    authToken?: string
  ): Promise<void> {
    if (this.isStreaming) return;

    this.callbacks = callbacks;
    this.reconnectAttempts = 0;
    this.audioQueue = []; // Clear queue on fresh start
    this.intentionalClose = false;
    this.sessionId = sessionId || null; // Use provided sessionId if available
    this.meetingId = meetingId || null;
    this.authToken = authToken || null;
    this.recordingStartTime = 0; // Will be set after AudioContext creation

    try {
      console.log('[AudioStream] Starting pipeline...');

      // 1. Get microphone access
      this.mediaStream = await navigator.mediaDevices.getUserMedia({
        audio: {
          echoCancellation: true,
          noiseSuppression: true,
          autoGainControl: true,
          channelCount: 1
        }
      });

      // 2. Setup Audio Context & Worklet
      this.audioContext = new AudioContext();
      await this.audioContext.audioWorklet.addModule('/audio-processor.worklet.js');

      // Track recording start time using AudioContext.currentTime (hardware-synced)
      this.recordingStartTime = this.audioContext.currentTime;
      console.log(`[AudioStream] Recording start time: ${this.recordingStartTime.toFixed(3)}s (AudioContext time)`);

      // 3. Connect WebSocket (with retry capability)
      await this.connectWithRetry();

      // 4. Create pipeline
      const source = this.audioContext.createMediaStreamSource(this.mediaStream);
      this.audioWorklet = new AudioWorkletNode(
        this.audioContext,
        'audio-stream-processor',
        { processorOptions: { sampleRate: this.audioContext.sampleRate } }
      );

      // 5. Handle audio data
      this.audioWorklet.port.onmessage = (event) => {
        const audioChunk = event.data as ArrayBuffer;

        // Use AudioContext.currentTime for hardware-synced timestamps
        // This is immune to network jitter and provides sub-millisecond precision
        if (!this.audioContext) return;

        const currentTime = this.audioContext.currentTime;
        const timestamp = currentTime - this.recordingStartTime;

        // Create combined buffer: [8 bytes timestamp] + [audio data]
        const combined = new ArrayBuffer(8 + audioChunk.byteLength);
        const view = new DataView(combined);

        // Write timestamp (Float64, Little Endian)
        // This is the SOURCE OF TRUTH for timing - client-side, hardware-synced
        view.setFloat64(0, timestamp, true);

        // Copy audio data
        new Int16Array(combined, 8).set(new Int16Array(audioChunk));

        // If connected, send immediately
        if (this.websocket?.readyState === WebSocket.OPEN) {
          this.flushQueue(); // Send any buffered data first
          this.websocket.send(combined);
        } else {
          // If disconnected, buffer the audio
          if (this.audioQueue.length >= this.maxBufferedChunks) {
            this.audioQueue.shift();
          }
          this.audioQueue.push(combined);

          if (this.audioQueue.length % 50 === 0) {
            console.warn(`[AudioStream] Buffering... Queue size: ${this.audioQueue.length}`);
          }
        }
      };

      source.connect(this.audioWorklet);
      this.isStreaming = true;
      console.log('[AudioStream] ✅ Streaming started');

    } catch (error) {
      console.error('[AudioStream] Start failed:', error);
      this.cleanup();
      throw error;
    }
  }

  private flushQueue() {
    if (this.audioQueue.length > 0 && this.websocket?.readyState === WebSocket.OPEN) {
      console.log(`[AudioStream] 🔄 Flushing ${this.audioQueue.length} buffered chunks`);
      while (this.audioQueue.length > 0) {
        const chunk = this.audioQueue.shift();
        if (chunk) this.websocket.send(chunk);
      }
    }
  }

  private async connectWithRetry(): Promise<void> {
    try {
      await this.connectWebSocket();
      this.reconnectAttempts = 0; // Reset on success
      this.isReconnecting = false;
      this.flushQueue();
    } catch (error) {
       if (this.reconnectAttempts < this.maxReconnectAttempts) {
         this.isReconnecting = true;
         this.reconnectAttempts++;
         const delay = Math.min(1000 * Math.pow(2, this.reconnectAttempts), 5000);
         console.warn(`[AudioStream] Connection failed. Retrying in ${delay}ms (Attempt ${this.reconnectAttempts}/${this.maxReconnectAttempts})`);
         
         await new Promise(r => setTimeout(r, delay));
         return this.connectWithRetry();
       } else {
         console.error('[AudioStream] Max retries reached. Giving up.');
         this.callbacks.onError?.(new Error('Connection lost. Please refresh.'));
         throw error;
       }
    }
  }

  private async connectWebSocket(): Promise<void> {
    return new Promise((resolve, reject) => {
      // Append session_id if we have one (for resuming)
      let url = this.sessionId 
        ? `${this.wsUrlOverride}?session_id=${this.sessionId}`
        : this.wsUrlOverride;
        
      if (this.meetingId) {
        url += (url.includes('?') ? '&' : '?') + `meeting_id=${encodeURIComponent(this.meetingId)}`;
      }
      if (this.authToken) {
        url += (url.includes('?') ? '&' : '?') + `auth_token=${encodeURIComponent(this.authToken)}`;
      }
        
      const ws = new WebSocket(url);
      ws.binaryType = 'arraybuffer';
      this.websocket = ws;

      const timeout = setTimeout(() => {
        if (this.websocket === ws && ws.readyState !== WebSocket.OPEN) {
          ws.close();
          reject(new Error('Timeout'));
        }
      }, 5000);

      ws.onopen = () => {
        if (this.websocket !== ws) return;
        clearTimeout(timeout);
        console.log('[AudioStream] WebSocket connected');
        
        // Start heartbeat
        this.startHeartbeat();
        
        resolve();
      };

      ws.onmessage = (event) => {
        if (this.websocket !== ws) return;
        try {
          const data = JSON.parse(event.data);
          
          if (data.type === 'connected') {
            console.log(`[AudioStream] ✅ Session ${data.session_id} ready`);
            this.sessionId = data.session_id; // Store for reconnection
            this.callbacks.onConnected?.(data.session_id);
          }
          else if (data.type === 'partial') this.callbacks.onPartial?.(data.text, data.confidence, data.is_stable);
          else if (data.type === 'final') {
            this.callbacks.onFinal?.(
              data.text, 
              data.confidence, 
              data.reason, 
              data.audio_start_time !== undefined ? {
                start: data.audio_start_time,
                end: data.audio_end_time,
                duration: data.duration
              } : undefined
            );
          }
          else if (data.type === 'stop_ack') {
            this.pendingStopResolve?.();
            this.pendingStopResolve = null;
            this.pendingStopReject = null;
          }
          else if (data.type === 'error') this.callbacks.onError?.(new Error(data.message), data.code);
        } catch (e) {
          console.error('Parse error', e);
        }
      };

      ws.onclose = (event) => {
        if (this.websocket !== ws) return;
        console.log(`[AudioStream] WebSocket closed: ${event.code}`);
        this.websocket = null;

        // If we were streaming and didn't close intentionally, try to reconnect.
        // Some browsers may mark closes as "clean" for server-initiated heartbeat shutdowns.
        const shouldReconnect =
          this.isStreaming &&
          !this.intentionalClose &&
          (event.code === 1000 || event.code === 1006 || event.code === 1008 || !event.wasClean);
        if (shouldReconnect && !this.reconnectInFlight) {
          this.reconnectInFlight = this.connectWithRetry()
            .catch(() => {
              void this.stop(); // Give up if retry fails
            })
            .finally(() => {
              this.reconnectInFlight = null;
            });
        }
        this.callbacks.onDisconnected?.();
      };

      ws.onerror = (err) => {
        if (this.websocket !== ws) return;
        // Just log, onclose will handle logic
        console.error('[AudioStream] WS Error:', err);
      };
    });
  }

  /**
   * Stop streaming
   */
  async stop(): Promise<void> {
    console.log('[AudioStream] Stopping...');
    this.intentionalClose = true;
    this.isStreaming = false;
    this.stopHeartbeat();
    await this.cleanup();
    console.log('[AudioStream] ✅ Stopped');
  }

  private startHeartbeat() {
    this.stopHeartbeat();
    this.heartbeatInterval = setInterval(() => {
      if (this.websocket?.readyState === WebSocket.OPEN) {
        try {
          this.websocket.send(JSON.stringify({ type: 'ping' }));
        } catch (error) {
          console.warn('[AudioStream] Heartbeat send failed:', error);
        }
      }
    }, 5000);
  }

  private stopHeartbeat() {
    if (this.heartbeatInterval) {
      clearInterval(this.heartbeatInterval);
      this.heartbeatInterval = null;
    }
  }

  /**
   * Cleanup all resources
   */
  private async cleanup(): Promise<void> {
    // Stop audio worklet
    if (this.audioWorklet) {
      this.audioWorklet.disconnect();
      this.audioWorklet = null;
    }

    // Close audio context
    if (this.audioContext) {
      this.audioContext.close();
      this.audioContext = null;
    }

    // Stop media stream
    if (this.mediaStream) {
      this.mediaStream.getTracks().forEach(track => track.stop());
      this.mediaStream = null;
    }

    // Close WebSocket
    if (this.websocket) {
      try {
        if (this.websocket.readyState === WebSocket.OPEN) {
          const ackPromise = new Promise<void>((resolve, reject) => {
            this.pendingStopResolve = resolve;
            this.pendingStopReject = reject;
          });
          this.websocket.send(JSON.stringify({ type: 'stop' }));
          await Promise.race([
            ackPromise,
            new Promise<void>((resolve) => setTimeout(resolve, 1500))
          ]);
          this.websocket.close(1000, 'client-stop');
        } else {
          this.websocket.close();
        }
      } catch {
        this.websocket.close();
      }
      this.websocket = null;
    }
    this.pendingStopResolve = null;
    this.pendingStopReject = null;
  }

  /**
   * Pause the audio stream by suspending the audio context
   */
  async pause(): Promise<void> {
    if (this.audioContext && this.audioContext.state === 'running') {
      console.log('[AudioStream] Pausing...');
      await this.audioContext.suspend();
      console.log('[AudioStream] Paused');
    }
  }

  /**
   * Resume the audio stream
   */
  async resume(): Promise<void> {
    if (this.audioContext && this.audioContext.state === 'suspended') {
      console.log('[AudioStream] Resuming...');
      await this.audioContext.resume();
      console.log('[AudioStream] Resumed');
    }
  }

  /**
   * Check if stream is paused
   */
  isPaused(): boolean {
    return this.audioContext?.state === 'suspended';
  }

  /**
   * Get current streaming status
   */
  isActive(): boolean {
    return this.isStreaming &&
           this.websocket?.readyState === WebSocket.OPEN &&
           this.audioContext?.state === 'running';
  }

  /**
   * Get the current session ID
   */
  getSessionId(): string | null {
    return this.sessionId;
  }
}
