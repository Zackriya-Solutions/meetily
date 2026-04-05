/**
 * IndexedDB Service for Transcript Recovery
 * Provides browser-based persistence for meeting transcripts and metadata
 * to enable recovery after app crashes or unexpected closures.
 */

// Database schema interfaces
export interface MeetingMetadata {
  meetingId: string;          // Primary key: "meeting-{timestamp}"
  title: string;              // Meeting title
  startTime: number;          // Unix timestamp (ms)
  lastUpdated: number;        // Unix timestamp (ms)
  transcriptCount: number;    // Number of transcript segments
  savedToSQLite: boolean;     // Flag: saved to backend DB
  folderPath?: string;        // Path to recording folder
}

export interface StoredTranscript {
  id?: number;                // Auto-increment primary key
  meetingId: string;          // Foreign key to meetings store
  text: string;               // Transcript text
  timestamp: string;          // ISO 8601 timestamp
  confidence: number;         // Whisper confidence score
  sequenceId: number;         // Sequence number for ordering
  storedAt: number;           // Unix timestamp when saved
  audio_start_time?: number;  // Recording-relative start time in seconds
  audio_end_time?: number;    // Recording-relative end time in seconds
  duration?: number;          // Duration in seconds
  chunk_start_time?: number;
  is_partial?: boolean;
}

interface TranscriptForStorage {
  text: string;
  timestamp: string;
  confidence: number;
  sequence_id: number;
  is_partial?: boolean;
  chunk_start_time?: number;
  audio_start_time?: number;
  audio_end_time?: number;
  duration?: number;
}

export class IndexedDBService {
  private db: IDBDatabase | null = null;
  private readonly DB_NAME = 'MeetFreeRecoveryDB';
  private readonly DB_VERSION = 1;
  private initPromise: Promise<void> | null = null;

  /**
   * Initialize database connection
   */
  async init(): Promise<void> {
    // Return existing promise if initialization is in progress
    if (this.initPromise) {
      return this.initPromise;
    }

    // Return immediately if already initialized
    if (this.db) {
      return Promise.resolve();
    }

    this.initPromise = new Promise((resolve, reject) => {
      try {
        const dbName = this.resolveDbName();
        const request = indexedDB.open(dbName, this.DB_VERSION);

        request.onerror = () => {
          console.error('Failed to open IndexedDB:', request.error);
          this.initPromise = null;
          reject(request.error);
        };

        request.onsuccess = () => {
          this.db = request.result;
          this.db.onversionchange = () => {
            this.db?.close();
            this.db = null;
            this.initPromise = null;
          };
          resolve();
        };

        request.onupgradeneeded = (event) => {
          const db = (event.target as IDBOpenDBRequest).result;

          // Create meetings store
          if (!db.objectStoreNames.contains('meetings')) {
            const meetingsStore = db.createObjectStore('meetings', { keyPath: 'meetingId' });
            meetingsStore.createIndex('lastUpdated', 'lastUpdated', { unique: false });
            meetingsStore.createIndex('savedToSQLite', 'savedToSQLite', { unique: false });
          }

          // Create transcripts store
          if (!db.objectStoreNames.contains('transcripts')) {
            const transcriptsStore = db.createObjectStore('transcripts', {
              keyPath: 'id',
              autoIncrement: true
            });
            transcriptsStore.createIndex('meetingId', 'meetingId', { unique: false });
            transcriptsStore.createIndex('storedAt', 'storedAt', { unique: false });
          }
        };
      } catch (error) {
        console.error('Exception during IndexedDB initialization:', error);
        this.initPromise = null;
        reject(error);
      }
    });

    return this.initPromise;
  }

  private resolveDbName(): string {
    return this.DB_NAME;
  }

  // Meeting operations

  /**
   * Save or update meeting metadata
   */
  async saveMeetingMetadata(metadata: MeetingMetadata): Promise<void> {
    try {
      if (!this.db) await this.init();

      const transaction = this.db!.transaction(['meetings'], 'readwrite');
      const store = transaction.objectStore('meetings');

      await new Promise<void>((resolve, reject) => {
        const request = store.put(metadata);
        request.onsuccess = () => resolve();
        request.onerror = () => reject(request.error);
      });
    } catch (error) {
      console.warn('Failed to save meeting metadata to IndexedDB:', error);
      // Fail silently - don't interrupt recording
    }
  }

  /**
   * Get meeting metadata by ID
   */
  async getMeetingMetadata(meetingId: string): Promise<MeetingMetadata | null> {
    try {
      if (!this.db) await this.init();

      const transaction = this.db!.transaction(['meetings'], 'readonly');
      const store = transaction.objectStore('meetings');

      return new Promise((resolve, reject) => {
        const request = store.get(meetingId);
        request.onsuccess = () => resolve(request.result || null);
        request.onerror = () => reject(request.error);
      });
    } catch (error) {
      console.error('Failed to get meeting metadata from IndexedDB:', error);
      return null;
    }
  }

  /**
   * Get all unsaved meetings (savedToSQLite = false)
   */
  async getAllMeetings(): Promise<MeetingMetadata[]> {
    try {
      if (!this.db) await this.init();

      const transaction = this.db!.transaction(['meetings'], 'readonly');
      const store = transaction.objectStore('meetings');

      return new Promise((resolve, reject) => {
        const request = store.getAll();
        request.onsuccess = () => {
          const allMeetings = request.result as MeetingMetadata[];
          // Filter for unsaved meetings (savedToSQLite = false)
          const unsavedMeetings = allMeetings.filter(m => m.savedToSQLite === false);

          // Sort by most recent first
          unsavedMeetings.sort((a, b) => b.lastUpdated - a.lastUpdated);
          resolve(unsavedMeetings);
        };
        request.onerror = () => reject(request.error);
      });
    } catch (error) {
      console.error('Failed to get meetings from IndexedDB:', error);
      return [];
    }
  }

  /**
   * Mark meeting as saved to SQLite
   */
  async markMeetingSaved(meetingId: string): Promise<void> {
    try {
      if (!this.db) await this.init();

      const transaction = this.db!.transaction(['meetings'], 'readwrite');
      const store = transaction.objectStore('meetings');

      return new Promise((resolve, reject) => {
        const getRequest = store.get(meetingId);
        getRequest.onsuccess = () => {
          const meeting = getRequest.result;
          if (meeting) {
            meeting.savedToSQLite = true;
            meeting.lastUpdated = Date.now();
            const putRequest = store.put(meeting);
            putRequest.onsuccess = () => resolve();
            putRequest.onerror = () => reject(putRequest.error);
          } else {
            resolve();
          }
        };
        getRequest.onerror = () => reject(getRequest.error);
      });
    } catch (error) {
      console.warn('Failed to mark meeting as saved:', error);
    }
  }

  /**
   * Delete meeting and all its transcripts
   */
  async deleteMeeting(meetingId: string): Promise<void> {
    try {
      if (!this.db) await this.init();

      const transaction = this.db!.transaction(['meetings', 'transcripts'], 'readwrite');
      const meetingsStore = transaction.objectStore('meetings');
      const transcriptsStore = transaction.objectStore('transcripts');

      // Delete transcripts
      await this.deleteTranscriptsForMeetingInternal(transcriptsStore, meetingId);

      // Delete meeting
      await new Promise<void>((resolve, reject) => {
        const request = meetingsStore.delete(meetingId);
        request.onsuccess = () => resolve();
        request.onerror = () => reject(request.error);
      });
    } catch (error) {
      console.error('Failed to delete meeting from IndexedDB:', error);
      throw error;
    }
  }

  // Transcript operations

  /**
   * Save a transcript segment
   */
  async saveTranscript(meetingId: string, transcript: TranscriptForStorage): Promise<void> {
    try {
      if (!this.db) await this.init();

      // Partial updates are transient and should not be persisted for crash recovery.
      if (transcript.is_partial) {
        return;
      }

      const storedTranscript: StoredTranscript = {
        text: transcript.text,
        timestamp: transcript.timestamp,
        confidence: transcript.confidence,
        sequenceId: transcript.sequence_id,
        chunk_start_time: transcript.chunk_start_time,
        audio_start_time: transcript.audio_start_time,
        audio_end_time: transcript.audio_end_time,
        duration: transcript.duration,
        is_partial: false,
        meetingId,
        storedAt: Date.now()
      };

      const transaction = this.db!.transaction(['transcripts', 'meetings'], 'readwrite');
      const transcriptsStore = transaction.objectStore('transcripts');
      const meetingsStore = transaction.objectStore('meetings');
      const transcriptIndex = transcriptsStore.index('meetingId');

      // Upsert by meeting + sequenceId to avoid duplicate segments in recovery.
      const existingTranscript = await new Promise<StoredTranscript | null>((resolve, reject) => {
        const request = transcriptIndex.openCursor(IDBKeyRange.only(meetingId));

        request.onsuccess = (event) => {
          const cursor = (event.target as IDBRequest<IDBCursorWithValue | null>).result;
          if (!cursor) {
            resolve(null);
            return;
          }

          const value = cursor.value as StoredTranscript;
          if (value.sequenceId === storedTranscript.sequenceId) {
            resolve({
              ...value,
              id: Number(cursor.primaryKey)
            });
            return;
          }

          cursor.continue();
        };

        request.onerror = () => reject(request.error);
      });

      const isNewTranscript = !existingTranscript;

      if (existingTranscript) {
        await new Promise<void>((resolve, reject) => {
          const request = transcriptsStore.put({
            ...existingTranscript,
            ...storedTranscript,
            id: existingTranscript.id,
          });
          request.onsuccess = () => resolve();
          request.onerror = () => reject(request.error);
        });
      } else {
        await new Promise<void>((resolve, reject) => {
          const request = transcriptsStore.add(storedTranscript);
          request.onsuccess = () => resolve();
          request.onerror = () => reject(request.error);
        });
      }

      // Update meeting metadata
      const meeting = await new Promise<MeetingMetadata | null>((resolve, reject) => {
        const request = meetingsStore.get(meetingId);
        request.onsuccess = () => resolve(request.result || null);
        request.onerror = () => reject(request.error);
      });

      if (meeting) {
        meeting.lastUpdated = Date.now();
        if (isNewTranscript) {
          meeting.transcriptCount = (meeting.transcriptCount || 0) + 1;
        }
        await new Promise<void>((resolve, reject) => {
          const request = meetingsStore.put(meeting);
          request.onsuccess = () => resolve();
          request.onerror = () => reject(request.error);
        });
      }
    } catch (error) {
      console.warn('Failed to save transcript to IndexedDB:', error);
      // Fail silently - don't interrupt recording
    }
  }

  /**
   * Get all transcripts for a meeting
   */
  async getTranscripts(meetingId: string): Promise<StoredTranscript[]> {
    try {
      if (!this.db) await this.init();

      const transaction = this.db!.transaction(['transcripts'], 'readonly');
      const store = transaction.objectStore('transcripts');
      const index = store.index('meetingId');

      return new Promise((resolve, reject) => {
        const request = index.getAll(meetingId);
        request.onsuccess = () => {
          const transcripts = request.result as StoredTranscript[];
          // Sort by sequence ID
          transcripts.sort((a, b) => a.sequenceId - b.sequenceId);
          resolve(transcripts);
        };
        request.onerror = () => reject(request.error);
      });
    } catch (error) {
      console.error('Failed to get transcripts from IndexedDB:', error);
      return [];
    }
  }

  /**
   * Get transcript count for a meeting
   */
  async getTranscriptCount(meetingId: string): Promise<number> {
    try {
      if (!this.db) await this.init();

      const transaction = this.db!.transaction(['transcripts'], 'readonly');
      const store = transaction.objectStore('transcripts');
      const index = store.index('meetingId');

      return new Promise((resolve, reject) => {
        const request = index.count(meetingId);
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject(request.error);
      });
    } catch (error) {
      console.error('Failed to get transcript count from IndexedDB:', error);
      return 0;
    }
  }

  // Cleanup operations

  /**
   * Delete meetings older than specified days
   * @param daysOld Number of days threshold
   * @returns Number of meetings deleted
   */
  async deleteOldMeetings(daysOld: number): Promise<number> {
    try {
      if (!this.db) await this.init();

      const cutoffTime = Date.now() - (daysOld * 24 * 60 * 60 * 1000);
      const transaction = this.db!.transaction(['meetings', 'transcripts'], 'readwrite');
      const meetingsStore = transaction.objectStore('meetings');
      const transcriptsStore = transaction.objectStore('transcripts');

      // Get all meetings
      const allMeetings = await new Promise<MeetingMetadata[]>((resolve, reject) => {
        const request = meetingsStore.getAll();
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject(request.error);
      });

      let deletedCount = 0;

      for (const meeting of allMeetings) {
        if (meeting.lastUpdated < cutoffTime) {
          // Delete transcripts
          await this.deleteTranscriptsForMeetingInternal(transcriptsStore, meeting.meetingId);

          // Delete meeting
          await new Promise<void>((resolve, reject) => {
            const request = meetingsStore.delete(meeting.meetingId);
            request.onsuccess = () => resolve();
            request.onerror = () => reject(request.error);
          });

          deletedCount++;
        }
      }

      console.log(`Cleaned up ${deletedCount} old meetings`);
      return deletedCount;
    } catch (error) {
      console.error('Failed to delete old meetings:', error);
      return 0;
    }
  }

  /**
   * Delete saved meetings older than specified hours
   * @param hoursOld Number of hours threshold after save
   * @returns Number of meetings deleted
   */
  async deleteSavedMeetings(hoursOld: number): Promise<number> {
    try {
      if (!this.db) await this.init();

      const cutoffTime = Date.now() - (hoursOld * 60 * 60 * 1000);
      const transaction = this.db!.transaction(['meetings', 'transcripts'], 'readwrite');
      const meetingsStore = transaction.objectStore('meetings');
      const transcriptsStore = transaction.objectStore('transcripts');

      // Get all meetings and filter for saved ones
      const allMeetings = await new Promise<MeetingMetadata[]>((resolve, reject) => {
        const request = meetingsStore.getAll();
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject(request.error);
      });

      // Filter for saved meetings (savedToSQLite = true)
      const savedMeetings = allMeetings.filter(m => m.savedToSQLite === true);

      let deletedCount = 0;

      for (const meeting of savedMeetings) {
        if (meeting.lastUpdated < cutoffTime) {
          // Delete transcripts
          await this.deleteTranscriptsForMeetingInternal(transcriptsStore, meeting.meetingId);

          // Delete meeting
          await new Promise<void>((resolve, reject) => {
            const request = meetingsStore.delete(meeting.meetingId);
            request.onsuccess = () => resolve();
            request.onerror = () => reject(request.error);
          });

          deletedCount++;
        }
      }

      console.log(`Cleaned up ${deletedCount} saved meetings`);
      return deletedCount;
    } catch (error) {
      console.error('Failed to delete saved meetings:', error);
      return 0;
    }
  }

  /**
   * Helper to delete all transcripts for a meeting
   */
  private async deleteTranscriptsForMeetingInternal(
    transcriptsStore: IDBObjectStore,
    meetingId: string
  ): Promise<void> {
    const index = transcriptsStore.index('meetingId');

    return new Promise((resolve, reject) => {
      const request = index.openCursor(IDBKeyRange.only(meetingId));

      request.onsuccess = (event) => {
        const cursor = (event.target as IDBRequest<IDBCursorWithValue>).result;
        if (cursor) {
          cursor.delete();
          cursor.continue();
        } else {
          resolve();
        }
      };

      request.onerror = () => reject(request.error);
    });
  }
}

// Export singleton instance
export const indexedDBService = new IndexedDBService();
