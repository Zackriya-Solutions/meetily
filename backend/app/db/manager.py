import asyncpg
import json
import os
import asyncio
from datetime import datetime
from typing import Optional, Dict, List
import logging
from contextlib import asynccontextmanager

# Import from core.encryption
try:
    from ..core.encryption import encrypt_key, decrypt_key
except ImportError:
    # Fallback for relative imports during local testing/script execution
    try:
        from ...core.encryption import encrypt_key, decrypt_key
    except ImportError:
        # Last resort if running from inside app/
        from core.encryption import encrypt_key, decrypt_key

logger = logging.getLogger(__name__)


class DatabaseManager:
    def __init__(self, db_url: str = None):
        if db_url is None:
            # SECURITY: Rely only on environment variable
            self.db_url = os.getenv("DATABASE_URL") or os.getenv("NEON_DATABASE_URL")
            if not self.db_url:
                raise ValueError(
                    "DATABASE_URL or NEON_DATABASE_URL environment variable is not set"
                )
        else:
            self.db_url = db_url

        # No more local init_db or schema validation on app startup
        # We assume the migration script has run or the DB is provisioned

    @asynccontextmanager
    async def _get_connection(self):
        """Get a new database connection from the pool"""
        # In a real prod app, you'd want a global pool created on startup
        # For now, creating a connection per request is okay for low traffic,
        # but we should move to a pool pattern in main.py startup event later.

        conn = None
        max_retries = 3
        retry_delay = 1
        last_error = None

        for attempt in range(max_retries):
            try:
                conn = await asyncpg.connect(self.db_url)
                break
            except (OSError, asyncpg.PostgresError) as e:
                last_error = e
                logger.warning(
                    f"Database connection attempt {attempt + 1}/{max_retries} failed: {e}"
                )
                if attempt < max_retries - 1:
                    await asyncio.sleep(retry_delay * (2**attempt))

        if conn is None:
            logger.error(f"Failed to connect to database after {max_retries} attempts")
            if last_error:
                raise last_error
            else:
                raise ConnectionError("Could not connect to database")

        try:
            yield conn
        finally:
            await conn.close()

    async def create_process(self, meeting_id: str) -> str:
        """Create a new process entry or update existing one and return its ID"""
        now = datetime.utcnow()  # Postgres expects datetime object, not string

        try:
            async with self._get_connection() as conn:
                async with conn.transaction():
                    # Upsert logic for Postgres
                    # Try update first
                    result = await conn.execute(
                        """
                        UPDATE summary_processes 
                        SET status = $1, updated_at = $2, start_time = $3, error = NULL, result = NULL
                        WHERE meeting_id = $4
                        """,
                        "PENDING",
                        now,
                        now,
                        meeting_id,
                    )

                    # Check if update happened (asyncpg returns "UPDATE N")
                    if result == "UPDATE 0":
                        await conn.execute(
                            """
                            INSERT INTO summary_processes (meeting_id, status, created_at, updated_at, start_time) 
                            VALUES ($1, $2, $3, $4, $5)
                            """,
                            meeting_id,
                            "PENDING",
                            now,
                            now,
                            now,
                        )

                    logger.info(
                        f"Successfully created/updated process for meeting_id: {meeting_id}"
                    )

        except Exception as e:
            logger.error(
                f"Database connection error in create_process: {str(e)}", exc_info=True
            )
            raise

        return meeting_id

    async def update_process(
        self,
        meeting_id: str,
        status: str,
        result: Optional[Dict] = None,
        error: Optional[str] = None,
        chunk_count: Optional[int] = None,
        processing_time: Optional[float] = None,
        metadata: Optional[Dict] = None,
    ):
        """Update a process status and result"""
        now = datetime.utcnow()

        try:
            async with self._get_connection() as conn:
                async with conn.transaction():
                    update_fields = ["status = $1", "updated_at = $2"]
                    params = [status, now]
                    param_idx = 3  # Start at $3

                    if result:
                        # Postgres JSONB handles dicts natively with asyncpg
                        update_fields.append(f"result = ${param_idx}")
                        params.append(
                            json.dumps(result)
                        )  # Store as JSON string for JSONB
                        param_idx += 1

                    if error:
                        sanitized_error = (
                            str(error).replace("\n", " ").replace("\r", "")[:1000]
                        )
                        update_fields.append(f"error = ${param_idx}")
                        params.append(sanitized_error)
                        param_idx += 1

                    if chunk_count is not None:
                        update_fields.append(f"chunk_count = ${param_idx}")
                        params.append(chunk_count)
                        param_idx += 1

                    if processing_time is not None:
                        update_fields.append(f"processing_time = ${param_idx}")
                        params.append(processing_time)
                        param_idx += 1

                    if metadata:
                        update_fields.append(f"metadata = ${param_idx}")
                        params.append(json.dumps(metadata))
                        param_idx += 1

                    if status.upper() in ["COMPLETED", "FAILED"]:
                        update_fields.append(f"end_time = ${param_idx}")
                        params.append(now)
                        param_idx += 1

                    params.append(meeting_id)
                    query = f"UPDATE summary_processes SET {', '.join(update_fields)} WHERE meeting_id = ${param_idx}"

                    res = await conn.execute(query, *params)
                    if res == "UPDATE 0":
                        logger.warning(
                            f"No process found to update for meeting_id: {meeting_id}"
                        )

                    logger.debug(
                        f"Successfully updated process status to {status} for meeting_id: {meeting_id}"
                    )

        except Exception as e:
            logger.error(
                f"Database connection error in update_process: {str(e)}", exc_info=True
            )
            raise

    async def save_transcript(
        self,
        meeting_id: str,
        transcript_text: str,
        model: str,
        model_name: str,
        chunk_size: int,
        overlap: int,
    ):
        """Save transcript data"""
        if not meeting_id or not meeting_id.strip():
            raise ValueError("meeting_id cannot be empty")
        if not transcript_text or not transcript_text.strip():
            raise ValueError("transcript_text cannot be empty")

        now = datetime.utcnow()

        try:
            async with self._get_connection() as conn:
                async with conn.transaction():
                    # Postgres upsert using ON CONFLICT
                    await conn.execute(
                        """
                        INSERT INTO full_transcripts (meeting_id, transcript_text, model, model_name, chunk_size, overlap, created_at)
                        VALUES ($1, $2, $3, $4, $5, $6, $7)
                        ON CONFLICT (meeting_id) 
                        DO UPDATE SET 
                            transcript_text = EXCLUDED.transcript_text,
                            model = EXCLUDED.model,
                            model_name = EXCLUDED.model_name,
                            chunk_size = EXCLUDED.chunk_size,
                            overlap = EXCLUDED.overlap,
                            created_at = EXCLUDED.created_at
                    """,
                        meeting_id,
                        transcript_text,
                        model,
                        model_name,
                        chunk_size,
                        overlap,
                        now,
                    )

                    logger.info(
                        f"Successfully saved transcript for meeting_id: {meeting_id} (size: {len(transcript_text)} chars)"
                    )

        except Exception as e:
            logger.error(
                f"Database connection error in save_transcript: {str(e)}", exc_info=True
            )
            raise

    async def update_meeting_name(self, meeting_id: str, meeting_name: str):
        """Update meeting name in both meetings and full_transcripts tables"""
        now = datetime.utcnow()
        async with self._get_connection() as conn:
            async with conn.transaction():
                await conn.execute(
                    """
                    UPDATE meetings
                    SET title = $1, updated_at = $2
                    WHERE id = $3
                """,
                    meeting_name,
                    now,
                    meeting_id,
                )

                await conn.execute(
                    """
                    UPDATE full_transcripts
                    SET meeting_name = $1
                    WHERE meeting_id = $2
                """,
                    meeting_name,
                    meeting_id,
                )

    async def get_transcript_data(self, meeting_id: str):
        """Get transcript/summary process data for a meeting"""
        async with self._get_connection() as conn:
            row = await conn.fetchrow(
                """
                SELECT meeting_id, status, result, error, start_time, end_time
                FROM summary_processes 
                WHERE meeting_id = $1
                ORDER BY start_time DESC
                LIMIT 1
            """,
                meeting_id,
            )

            if row:
                # Convert Record to dict
                data = dict(row)
                # Handle JSONB fields if they are strings (asyncpg might return dict directly if jsonb)
                if isinstance(data.get("result"), str):
                    try:
                        data["result"] = json.loads(data["result"])
                    except:
                        pass
                return data
            return None

    async def save_meeting(
        self,
        meeting_id: str,
        title: str,
        folder_path: str = None,
        owner_id: str = None,
        workspace_id: str = None,
    ):
        """Save or update a meeting"""
        try:
            async with self._get_connection() as conn:
                # Check existence
                exists = await conn.fetchval(
                    "SELECT id FROM meetings WHERE id = $1", meeting_id
                )

                if not exists:
                    now = datetime.utcnow()
                    await conn.execute(
                        """
                        INSERT INTO meetings (id, title, created_at, updated_at, folder_path, owner_id, workspace_id)
                        VALUES ($1, $2, $3, $4, $5, $6, $7)
                    """,
                        meeting_id,
                        title,
                        now,
                        now,
                        folder_path,
                        owner_id,
                        workspace_id,
                    )
                    logger.info(
                        f"Saved meeting {meeting_id} (Owner: {owner_id}, WS: {workspace_id})"
                    )
                else:
                    # Optional: We could update title here if we wanted
                    pass
                return True
        except Exception as e:
            logger.error(f"Error saving meeting: {str(e)}")
            raise

    async def save_meeting_transcript(
        self,
        meeting_id: str,
        transcript: str,
        timestamp: str,
        summary: str = "",
        action_items: str = "",
        key_points: str = "",
        audio_start_time: float = None,
        audio_end_time: float = None,
        duration: float = None,
        source: str = "live",
        speaker: str = None,
        speaker_confidence: float = None,
    ):
        """Save a transcript for a meeting"""
        try:
            async with self._get_connection() as conn:
                # No ON CONFLICT logic needed as transcripts table has SERIAL ID, duplicates allowed unless unique constraint
                await conn.execute(
                    """
                    INSERT INTO transcript_segments (
                        meeting_id, transcript, timestamp, summary, action_items, key_points,
                        audio_start_time, audio_end_time, duration, source, speaker, speaker_confidence
                    ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
                """,
                    meeting_id,
                    transcript,
                    timestamp,
                    summary,
                    action_items,
                    key_points,
                    audio_start_time,
                    audio_end_time,
                    duration,
                    source,
                    speaker,
                    speaker_confidence,
                )
                return True
        except Exception as e:
            logger.error(f"Error saving transcript: {str(e)}")
            raise

    async def save_meeting_transcripts_batch(self, meeting_id: str, transcripts: list):
        """Batch save transcripts for a meeting"""
        if not transcripts:
            return True

        try:
            async with self._get_connection() as conn:
                # Prepare data for executemany
                data = [
                    (
                        meeting_id,
                        t.text,
                        t.timestamp,
                        "",  # summary
                        "",  # action_items
                        "",  # key_points
                        t.audio_start_time,
                        t.audio_end_time,
                        t.duration,
                        "web_client",  # source
                        None,  # speaker
                        None,  # speaker_confidence
                    )
                    for t in transcripts
                ]

                await conn.executemany(
                    """
                    INSERT INTO transcript_segments (
                        meeting_id, transcript, timestamp, summary, action_items, key_points,
                        audio_start_time, audio_end_time, duration, source, speaker, speaker_confidence
                    ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
                    """,
                    data,
                )
                return True
        except Exception as e:
            logger.error(f"Error batch saving transcripts: {str(e)}")
            raise

    async def save_transcript_version(
        self,
        meeting_id: str,
        source: str,
        content: list,
        is_authoritative: bool = False,
        alignment_config: Optional[Dict] = None,
        created_by: str = "system",
    ) -> int:
        """
        Save a full transcript version snapshot with auto-incrementing version number.

        Args:
            meeting_id: Meeting ID
            source: 'live' | 'diarized' | 'manual_edit'
            content: Array of transcript segments
            is_authoritative: If True, demotes previous authoritative version
            alignment_config: Alignment algorithm settings used
            created_by: 'system' or user email

        Returns:
            Version number (int)
        """
        try:
            async with self._get_connection() as conn:
                async with conn.transaction():
                    # Get next version number
                    version_num = await conn.fetchval(
                        """
                        SELECT COALESCE(MAX(version_num), 0) + 1
                        FROM transcript_versions
                        WHERE meeting_id = $1
                    """,
                        meeting_id,
                    )

                    # Calculate confidence metrics from content
                    confidence_metrics = self._calculate_confidence_metrics(content)

                    # If making this authoritative, demote previous
                    if is_authoritative:
                        await conn.execute(
                            """
                            UPDATE transcript_versions
                            SET is_authoritative = FALSE
                            WHERE meeting_id = $1 AND is_authoritative = TRUE
                        """,
                            meeting_id,
                        )

                    # Insert new version
                    await conn.execute(
                        """
                        INSERT INTO transcript_versions (
                            meeting_id, version_num, source, content_json,
                            is_authoritative, created_by, alignment_config, confidence_metrics
                        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                    """,
                        meeting_id,
                        version_num,
                        source,
                        json.dumps(content, default=str),  # Handle datetimes
                        is_authoritative,
                        created_by,
                        json.dumps(alignment_config or {}),
                        json.dumps(confidence_metrics),
                    )

                    logger.info(
                        f"Saved transcript version v{version_num} for {meeting_id} "
                        f"(source={source}, auth={is_authoritative}, "
                        f"avg_conf={confidence_metrics.get('avg_confidence', 0):.2f})"
                    )
                    return version_num
        except Exception as e:
            logger.error(f"Error saving transcript version: {str(e)}")
            raise

    def _calculate_confidence_metrics(self, segments: List[Dict]) -> Dict:
        """Calculate confidence metrics from transcript segments."""
        if not segments:
            return {
                "total_segments": 0,
                "avg_confidence": 0.0,
                "confident_count": 0,
                "uncertain_count": 0,
                "overlap_count": 0,
            }

        total_confidence = 0.0
        confident_count = 0
        uncertain_count = 0
        overlap_count = 0

        for seg in segments:
            # Handle potential None values safely
            speaker_conf = seg.get("speaker_confidence")
            if speaker_conf is None:
                speaker_conf = seg.get("confidence", 1.0)

            # If still None (unlikely but possible if confidence key exists but is None), default to 1.0
            if speaker_conf is None:
                speaker_conf = 1.0

            total_confidence += float(speaker_conf)

            state = seg.get("alignment_state", "CONFIDENT")
            if state == "CONFIDENT":
                confident_count += 1
            elif state == "UNCERTAIN":
                uncertain_count += 1
            elif state == "OVERLAP":
                overlap_count += 1

        return {
            "total_segments": len(segments),
            "avg_confidence": total_confidence / len(segments),
            "confident_count": confident_count,
            "uncertain_count": uncertain_count,
            "overlap_count": overlap_count,
        }

    async def get_transcript_versions(self, meeting_id: str) -> List[Dict]:
        """Get all versions for a meeting, ordered by version number."""
        async with self._get_connection() as conn:
            rows = await conn.fetch(
                """
                SELECT version_num, source, is_authoritative, created_at,
                       confidence_metrics, created_by
                FROM transcript_versions
                WHERE meeting_id = $1
                ORDER BY version_num DESC
            """,
                meeting_id,
            )

            return [
                {
                    "version_num": row["version_num"],
                    "source": row["source"],
                    "is_authoritative": row["is_authoritative"],
                    "created_at": row["created_at"].isoformat()
                    if row["created_at"]
                    else None,
                    "confidence_metrics": json.loads(row["confidence_metrics"])
                    if isinstance(row["confidence_metrics"], str)
                    else row["confidence_metrics"],
                    "created_by": row["created_by"],
                }
                for row in rows
            ]

    async def get_transcript_version_content(
        self, meeting_id: str, version_num: int
    ) -> Optional[List[Dict]]:
        """Get the content of a specific transcript version."""
        async with self._get_connection() as conn:
            content_json = await conn.fetchval(
                """
                SELECT content_json
                FROM transcript_versions
                WHERE meeting_id = $1 AND version_num = $2
            """,
                meeting_id,
                version_num,
            )

            if content_json:
                if isinstance(content_json, str):
                    return json.loads(content_json)
                return content_json
            return None

    async def delete_transcript_version(
        self, meeting_id: str, version_num: int
    ) -> bool:
        """Delete a specific transcript version snapshot."""
        try:
            async with self._get_connection() as conn:
                result = await conn.execute(
                    """
                    DELETE FROM transcript_versions
                    WHERE meeting_id = $1 AND version_num = $2
                """,
                    meeting_id,
                    version_num,
                )

                if result == "DELETE 0":
                    return False

                logger.info(f"Deleted version v{version_num} for meeting {meeting_id}")
                return True
        except Exception as e:
            logger.error(f"Error deleting transcript version: {str(e)}")
            raise

    async def clear_meeting_transcripts(self, meeting_id: str):
        """Delete all transcript segments for a meeting"""
        try:
            async with self._get_connection() as conn:
                await conn.execute(
                    "DELETE FROM transcript_segments WHERE meeting_id = $1", meeting_id
                )
                logger.info(f"Cleared transcripts for meeting {meeting_id}")
                return True
        except Exception as e:
            logger.error(f"Error clearing transcripts: {str(e)}")
            raise

    async def get_meeting(self, meeting_id: str):
        """Get a meeting by ID with all its transcripts"""
        try:
            async with self._get_connection() as conn:
                # Get meeting details
                meeting = await conn.fetchrow(
                    """
                    SELECT id, title, created_at, updated_at, owner_id, workspace_id
                    FROM meetings
                    WHERE id = $1
                """,
                    meeting_id,
                )

                if not meeting:
                    return None

                # Get transcripts
                transcripts = await conn.fetch(
                    """
                    SELECT id, transcript, timestamp, audio_start_time, audio_end_time, duration, speaker, speaker_confidence, source, alignment_state
                    FROM transcript_segments
                    WHERE meeting_id = $1
                      AND (source IS NULL OR source != 'diarized')
                    ORDER BY id ASC
                """,
                    meeting_id,
                )

                return {
                    "id": meeting["id"],
                    "title": meeting["title"],
                    "created_at": meeting["created_at"].isoformat()
                    if meeting["created_at"]
                    else None,
                    "updated_at": meeting["updated_at"].isoformat()
                    if meeting["updated_at"]
                    else None,
                    "owner_id": meeting["owner_id"],
                    "workspace_id": meeting["workspace_id"],
                    "transcripts": [
                        {
                            "id": str(t["id"]),
                            "text": t["transcript"],
                            "timestamp": t["timestamp"],
                            "audio_start_time": t["audio_start_time"],
                            "audio_end_time": t["audio_end_time"],
                            "duration": t["duration"],
                            "speaker": t["speaker"],
                            "speaker_confidence": t["speaker_confidence"],
                            "source": t["source"],
                            "alignment_state": t["alignment_state"],
                        }
                        for t in transcripts
                    ],
                }
        except Exception as e:
            logger.error(f"Error getting meeting: {str(e)}")
            raise

    async def get_full_transcript_text(self, meeting_id: str):
        """Get the full transcript text from full_transcripts table"""
        try:
            async with self._get_connection() as conn:
                row = await conn.fetchrow(
                    """
                    SELECT transcript_text
                    FROM full_transcripts
                    WHERE meeting_id = $1
                """,
                    meeting_id,
                )
                return row["transcript_text"] if row else None
        except Exception as e:
            logger.error(f"Error getting full transcript: {str(e)}")
            return None

    async def update_meeting_title(self, meeting_id: str, new_title: str):
        """Update a meeting's title"""
        now = datetime.utcnow()
        async with self._get_connection() as conn:
            await conn.execute(
                """
                UPDATE meetings
                SET title = $1, updated_at = $2
                WHERE id = $3
            """,
                new_title,
                now,
                meeting_id,
            )

    async def get_all_meetings(self):
        """Get all meetings with basic information"""
        async with self._get_connection() as conn:
            rows = await conn.fetch("""
                SELECT id, title, created_at, owner_id, workspace_id
                FROM meetings
                ORDER BY created_at DESC
            """)
            return [
                {
                    "id": row["id"],
                    "title": row["title"],
                    "created_at": row["created_at"].isoformat()
                    if row["created_at"]
                    else None,
                    "owner_id": row["owner_id"],
                    "workspace_id": row["workspace_id"],
                }
                for row in rows
            ]

    async def delete_meeting(self, meeting_id: str):
        """Delete a meeting and all its associated data"""
        if not meeting_id or not meeting_id.strip():
            raise ValueError("meeting_id cannot be empty")

        try:
            async with self._get_connection() as conn:
                # Postgres CASCADE delete handles dependent rows if configured in FKs.
                # Our migration script added ON DELETE CASCADE, so we just delete from meetings.
                result = await conn.execute(
                    "DELETE FROM meetings WHERE id = $1", meeting_id
                )

                if result == "DELETE 0":
                    logger.warning(f"Meeting {meeting_id} not found for deletion")
                    return False

                logger.info(f"Successfully deleted meeting {meeting_id} (and cascaded)")
                return True

        except Exception as e:
            logger.error(
                f"Database connection error in delete_meeting: {str(e)}", exc_info=True
            )
            return False

    async def get_model_config(self):
        """Get the current model configuration"""
        async with self._get_connection() as conn:
            # Postgres column is likely lowercase 'whispermodel'
            row = await conn.fetchrow(
                "SELECT provider, model, whisperModel FROM settings WHERE id = '1'"
            )
            if row:
                return {
                    "provider": row["provider"],
                    "model": row["model"],
                    "whisperModel": row["whispermodel"],
                }
            # Default to Gemini if no config found
            return {
                "provider": "gemini",
                "model": "gemini-2.5-flash",
                "whisperModel": "large-v3",
            }

    async def save_model_config(self, provider: str, model: str, whisperModel: str):
        """Save the model configuration"""
        try:
            async with self._get_connection() as conn:
                # Upsert settings (assuming id='1' is the singleton config)
                # Use unquoted whisperModel to match lowercase column
                await conn.execute(
                    """
                    INSERT INTO settings (id, provider, model, whisperModel)
                    VALUES ($1, $2, $3, $4)
                    ON CONFLICT (id) DO UPDATE SET
                        provider = EXCLUDED.provider,
                        model = EXCLUDED.model,
                        whisperModel = EXCLUDED.whisperModel
                """,
                    "1",
                    provider,
                    model,
                    whisperModel,
                )

                logger.info(
                    f"Successfully saved model configuration: {provider}/{model}"
                )
        except Exception as e:
            logger.error(f"Failed to save model configuration: {str(e)}", exc_info=True)
            raise

    async def save_api_key(self, api_key: str, provider: str):
        """Save the API key"""
        provider_map = {
            "openai": "openaiapikey",
            "claude": "anthropicapikey",
            "groq": "groqapikey",
            "ollama": "ollamaapikey",
            "gemini": "geminiapikey",
        }
        if provider not in provider_map:
            raise ValueError(f"Invalid provider: {provider}")

        column_name = provider_map[provider]

        try:
            async with self._get_connection() as conn:
                # Check if row exists, if not insert default, then update
                # Or just Upsert with COALESCE for other fields?
                # Simpler: Upsert a new row if not exists, then update specific column

                # Ensure row 1 exists
                await conn.execute("""
                    INSERT INTO settings (id, provider, model, whisperModel)
                    VALUES ('1', 'openai', 'gpt-4o', 'large-v3')
                    ON CONFLICT (id) DO NOTHING
                """)

                # Update specific key
                # Note: We can't use dynamic column name in execute params, must be f-string safely
                # column_name is from a safe whitelist above.
                await conn.execute(
                    f"""
                    UPDATE settings SET "{column_name}" = $1 WHERE id = '1'
                """,
                    api_key,
                )

                logger.info(f"Successfully saved API key for provider: {provider}")
        except Exception as e:
            logger.error(
                f"Failed to save API key for provider {provider}: {str(e)}",
                exc_info=True,
            )
            raise

    async def get_api_key(self, provider: str, user_email: Optional[str] = None):
        """Get the API key"""
        if user_email:
            user_key = await self.get_user_api_key(user_email, provider)
            if user_key:
                return user_key

        provider_map = {
            "openai": "openaiapikey",
            "claude": "anthropicapikey",
            "groq": "groqapikey",
            "ollama": "ollamaapikey",
            "gemini": "geminiapikey",
            "deepgram": "deepgramapikey",
        }
        if provider not in provider_map:
            return ""

        column_name = provider_map[provider]
        async with self._get_connection() as conn:
            val = await conn.fetchval(
                f"SELECT \"{column_name}\" FROM settings WHERE id = '1'"
            )
            return val if val else ""

    async def save_user_api_key(self, user_email: str, provider: str, api_key: str):
        """Save an encrypted API key for a specific user."""
        encrypted_key = encrypt_key(api_key)
        now = datetime.utcnow()
        async with self._get_connection() as conn:
            await conn.execute(
                """
                INSERT INTO user_api_keys (user_email, provider, api_key, updated_at)
                VALUES ($1, $2, $3, $4)
                ON CONFLICT(user_email, provider) DO UPDATE SET
                    api_key = EXCLUDED.api_key,
                    updated_at = EXCLUDED.updated_at
            """,
                user_email,
                provider,
                encrypted_key,
                now,
            )

    async def get_user_api_key(self, user_email: str, provider: str) -> Optional[str]:
        """Retrieve and decrypt an API key for a specific user."""
        async with self._get_connection() as conn:
            encrypted_key = await conn.fetchval(
                "SELECT api_key FROM user_api_keys WHERE user_email = $1 AND provider = $2 AND is_active = TRUE",
                user_email,
                provider,
            )
            if encrypted_key:
                return decrypt_key(encrypted_key)
        return None

    async def get_user_api_keys(self, user_email: str) -> Dict[str, str]:
        """Retrieve all active API keys for a specific user (returns masked keys)."""
        keys = {}
        async with self._get_connection() as conn:
            rows = await conn.fetch(
                "SELECT provider, api_key FROM user_api_keys WHERE user_email = $1 AND is_active = TRUE",
                user_email,
            )
            for row in rows:
                provider = row["provider"]
                encrypted_key = row["api_key"]
                decrypted = decrypt_key(encrypted_key)
                if decrypted and len(decrypted) > 8:
                    keys[provider] = f"{decrypted[:4]}...{decrypted[-4:]}"
                else:
                    keys[provider] = "****"
        return keys

    async def delete_user_api_key(self, user_email: str, provider: str):
        """Remove an API key for a specific user."""
        async with self._get_connection() as conn:
            await conn.execute(
                "DELETE FROM user_api_keys WHERE user_email = $1 AND provider = $2",
                user_email,
                provider,
            )

    async def get_transcript_config(self):
        """Get the current transcript configuration"""
        async with self._get_connection() as conn:
            row = await conn.fetchrow("SELECT provider, model FROM transcript_settings")
            if row:
                return {"provider": row["provider"], "model": row["model"]}
            return {"provider": "localWhisper", "model": "large-v3"}

    async def save_transcript_config(self, provider: str, model: str):
        """Save the transcript settings"""
        try:
            async with self._get_connection() as conn:
                await conn.execute(
                    """
                    INSERT INTO transcript_settings (id, provider, model)
                    VALUES ('1', $1, $2)
                    ON CONFLICT (id) DO UPDATE SET
                        provider = EXCLUDED.provider,
                        model = EXCLUDED.model
                """,
                    provider,
                    model,
                )
                logger.info(
                    f"Successfully saved transcript configuration: {provider}/{model}"
                )
        except Exception as e:
            logger.error(
                f"Failed to save transcript configuration: {str(e)}", exc_info=True
            )
            raise

    async def save_transcript_api_key(self, api_key: str, provider: str):
        """Save the transcript API key"""
        provider_map = {
            "localWhisper": "whisperapikey",
            "deepgram": "deepgramapikey",
            "elevenLabs": "elevenlabsapikey",
            "groq": "groqapikey",
            "openai": "openaiapikey",
        }
        if provider not in provider_map:
            raise ValueError(f"Invalid provider: {provider}")

        column_name = provider_map[provider]

        try:
            async with self._get_connection() as conn:
                await conn.execute(
                    "INSERT INTO transcript_settings (id, provider, model) VALUES ('1', 'localWhisper', 'large-v3') ON CONFLICT (id) DO NOTHING"
                )

                await conn.execute(
                    f"""
                    UPDATE transcript_settings SET "{column_name}" = $1 WHERE id = '1'
                """,
                    api_key,
                )

                logger.info(
                    f"Successfully saved transcript API key for provider: {provider}"
                )
        except Exception as e:
            logger.error(
                f"Failed to save transcript API key for provider {provider}: {str(e)}",
                exc_info=True,
            )
            raise

    async def get_transcript_api_key(
        self, provider: str, user_email: Optional[str] = None
    ):
        """Get the transcript API key"""
        if user_email:
            user_key = await self.get_user_api_key(user_email, provider)
            if user_key:
                return user_key

        provider_map = {
            "localWhisper": "whisperapikey",
            "deepgram": "deepgramapikey",
            "elevenLabs": "elevenlabsapikey",
            "groq": "groqapikey",
            "openai": "openaiapikey",
        }
        if provider not in provider_map:
            raise ValueError(f"Invalid provider: {provider}")

        column_name = provider_map[provider]
        async with self._get_connection() as conn:
            val = await conn.fetchval(
                f"SELECT \"{column_name}\" FROM transcript_settings WHERE id = '1'"
            )
            return val if val else ""

    async def save_calendar_oauth_state(
        self, state: str, user_email: str, code_verifier: str, expires_at: datetime
    ):
        async with self._get_connection() as conn:
            await conn.execute(
                """
                INSERT INTO calendar_oauth_states (state, user_email, code_verifier, expires_at, created_at)
                VALUES ($1, $2, $3, $4, $5)
            """,
                state,
                user_email,
                code_verifier,
                expires_at,
                datetime.utcnow(),
            )

    async def consume_calendar_oauth_state(self, state: str) -> Optional[Dict]:
        async with self._get_connection() as conn:
            async with conn.transaction():
                row = await conn.fetchrow(
                    """
                    SELECT state, user_email, code_verifier, expires_at
                    FROM calendar_oauth_states
                    WHERE state = $1
                """,
                    state,
                )
                if not row:
                    return None

                await conn.execute(
                    "DELETE FROM calendar_oauth_states WHERE state = $1", state
                )

                if row["expires_at"] and row["expires_at"] < datetime.utcnow():
                    return None

                return {
                    "state": row["state"],
                    "user_email": row["user_email"],
                    "code_verifier": row["code_verifier"],
                }

    async def upsert_calendar_integration(
        self,
        user_email: str,
        provider: str,
        external_account_email: str,
        scopes: List[str],
        access_token: str,
        refresh_token: str,
        token_expires_at: Optional[datetime] = None,
    ):
        encrypted_access_token = encrypt_key(access_token)
        encrypted_refresh_token = encrypt_key(refresh_token)
        now = datetime.utcnow()

        async with self._get_connection() as conn:
            await conn.execute(
                """
                INSERT INTO calendar_integrations (
                    user_email, provider, external_account_email, scopes,
                    access_token, refresh_token, token_expires_at, is_active, connected_at, updated_at
                )
                VALUES ($1, $2, $3, $4::jsonb, $5, $6, $7, TRUE, $8, $8)
                ON CONFLICT (user_email, provider) DO UPDATE SET
                    external_account_email = EXCLUDED.external_account_email,
                    scopes = EXCLUDED.scopes,
                    access_token = EXCLUDED.access_token,
                    refresh_token = EXCLUDED.refresh_token,
                    token_expires_at = EXCLUDED.token_expires_at,
                    is_active = TRUE,
                    connected_at = EXCLUDED.connected_at,
                    updated_at = EXCLUDED.updated_at
            """,
                user_email,
                provider,
                external_account_email,
                json.dumps(scopes),
                encrypted_access_token,
                encrypted_refresh_token,
                token_expires_at,
                now,
            )

    async def disconnect_calendar_integration(self, user_email: str, provider: str):
        async with self._get_connection() as conn:
            await conn.execute(
                """
                UPDATE calendar_integrations
                SET is_active = FALSE,
                    access_token = '',
                    refresh_token = '',
                    token_expires_at = NULL,
                    updated_at = $3
                WHERE user_email = $1 AND provider = $2
            """,
                user_email,
                provider,
                datetime.utcnow(),
            )

    async def get_calendar_integration(self, user_email: str, provider: str) -> Dict:
        async with self._get_connection() as conn:
            row = await conn.fetchrow(
                """
                SELECT provider, external_account_email, scopes, is_active, connected_at
                FROM calendar_integrations
                WHERE user_email = $1 AND provider = $2
            """,
                user_email,
                provider,
            )

            if not row:
                return {
                    "provider": provider,
                    "connected": False,
                    "account_email": None,
                    "connected_at": None,
                    "scopes": [],
                    "can_writeback": False,
                }

            raw_scopes = row["scopes"] or []
            scopes = json.loads(raw_scopes) if isinstance(raw_scopes, str) else raw_scopes
            can_writeback = "https://www.googleapis.com/auth/calendar.events" in scopes

            return {
                "provider": row["provider"],
                "connected": bool(row["is_active"]),
                "account_email": row["external_account_email"],
                "connected_at": row["connected_at"].isoformat()
                if row["connected_at"]
                else None,
                "scopes": scopes,
                "can_writeback": can_writeback,
            }

    async def get_calendar_automation_settings(self, user_email: str) -> Dict:
        defaults = {
            "reminders_enabled": True,
            "attendee_reminders_enabled": False,
            "reminder_offset_minutes": 2,
            "recap_enabled": True,
            "writeback_enabled": False,
        }

        async with self._get_connection() as conn:
            row = await conn.fetchrow(
                """
                SELECT reminders_enabled, attendee_reminders_enabled, reminder_offset_minutes,
                       recap_enabled, writeback_enabled
                FROM calendar_automation_settings
                WHERE user_email = $1
            """,
                user_email,
            )
            if not row:
                return defaults
            return {
                "reminders_enabled": row["reminders_enabled"],
                "attendee_reminders_enabled": row["attendee_reminders_enabled"],
                "reminder_offset_minutes": row["reminder_offset_minutes"],
                "recap_enabled": row["recap_enabled"],
                "writeback_enabled": row["writeback_enabled"],
            }

    async def upsert_calendar_automation_settings(
        self, user_email: str, settings: Dict
    ) -> Dict:
        now = datetime.utcnow()
        async with self._get_connection() as conn:
            row = await conn.fetchrow(
                """
                INSERT INTO calendar_automation_settings (
                    user_email, reminders_enabled, attendee_reminders_enabled,
                    reminder_offset_minutes, recap_enabled, writeback_enabled, updated_at
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7)
                ON CONFLICT (user_email) DO UPDATE SET
                    reminders_enabled = EXCLUDED.reminders_enabled,
                    attendee_reminders_enabled = EXCLUDED.attendee_reminders_enabled,
                    reminder_offset_minutes = EXCLUDED.reminder_offset_minutes,
                    recap_enabled = EXCLUDED.recap_enabled,
                    writeback_enabled = EXCLUDED.writeback_enabled,
                    updated_at = EXCLUDED.updated_at
                RETURNING reminders_enabled, attendee_reminders_enabled, reminder_offset_minutes,
                          recap_enabled, writeback_enabled
            """,
                user_email,
                settings["reminders_enabled"],
                settings["attendee_reminders_enabled"],
                settings["reminder_offset_minutes"],
                settings["recap_enabled"],
                settings["writeback_enabled"],
                now,
            )
            return {
                "reminders_enabled": row["reminders_enabled"],
                "attendee_reminders_enabled": row["attendee_reminders_enabled"],
                "reminder_offset_minutes": row["reminder_offset_minutes"],
                "recap_enabled": row["recap_enabled"],
                "writeback_enabled": row["writeback_enabled"],
            }

    async def get_active_calendar_integrations(self, provider: str = "google") -> List[Dict]:
        async with self._get_connection() as conn:
            rows = await conn.fetch(
                """
                SELECT user_email, provider, external_account_email, scopes,
                       access_token, refresh_token, token_expires_at, is_active
                FROM calendar_integrations
                WHERE provider = $1 AND is_active = TRUE
            """,
                provider,
            )

            integrations = []
            for row in rows:
                raw_scopes = row["scopes"] or []
                scopes = (
                    json.loads(raw_scopes)
                    if isinstance(raw_scopes, str)
                    else raw_scopes
                )
                integrations.append(
                    {
                        "user_email": row["user_email"],
                        "provider": row["provider"],
                        "external_account_email": row["external_account_email"],
                        "scopes": scopes,
                        "access_token": decrypt_key(row["access_token"]),
                        "refresh_token": decrypt_key(row["refresh_token"]),
                        "token_expires_at": row["token_expires_at"],
                    }
                )
            return integrations

    async def update_calendar_access_token(
        self,
        user_email: str,
        provider: str,
        access_token: str,
        token_expires_at: Optional[datetime],
    ):
        encrypted_access_token = encrypt_key(access_token)
        async with self._get_connection() as conn:
            await conn.execute(
                """
                UPDATE calendar_integrations
                SET access_token = $3,
                    token_expires_at = $4,
                    updated_at = $5
                WHERE user_email = $1 AND provider = $2
            """,
                user_email,
                provider,
                encrypted_access_token,
                token_expires_at,
                datetime.utcnow(),
            )

    async def upsert_calendar_events(
        self,
        user_email: str,
        provider: str,
        events: List[Dict],
    ):
        if not events:
            return

        now = datetime.utcnow()
        async with self._get_connection() as conn:
            for event in events:
                await conn.execute(
                    """
                    DELETE FROM calendar_events
                    WHERE user_email = $1
                      AND provider = $2
                      AND event_id = $3
                      AND start_time <> $4
                """,
                    user_email,
                    provider,
                    event["event_id"],
                    event["start_time"],
                )
                await conn.execute(
                    """
                    INSERT INTO calendar_events (
                        user_email, provider, event_id, meeting_title, meeting_link,
                        organizer_email, attendee_emails, start_time, end_time, updated_at
                    )
                    VALUES ($1, $2, $3, $4, $5, $6, $7::jsonb, $8, $9, $10)
                    ON CONFLICT (user_email, provider, event_id, start_time) DO UPDATE SET
                        meeting_title = EXCLUDED.meeting_title,
                        meeting_link = EXCLUDED.meeting_link,
                        organizer_email = EXCLUDED.organizer_email,
                        attendee_emails = EXCLUDED.attendee_emails,
                        end_time = EXCLUDED.end_time,
                        updated_at = EXCLUDED.updated_at
                """,
                    user_email,
                    provider,
                    event["event_id"],
                    event["meeting_title"],
                    event.get("meeting_link"),
                    event.get("organizer_email"),
                    json.dumps(event.get("attendee_emails", [])),
                    event["start_time"],
                    event.get("end_time"),
                    now,
                )

    async def get_due_calendar_reminders(self) -> List[Dict]:
        async with self._get_connection() as conn:
            rows = await conn.fetch(
                """
                SELECT e.user_email, e.provider, e.event_id, e.meeting_title, e.meeting_link,
                       e.attendee_emails, e.start_time,
                       COALESCE(s.attendee_reminders_enabled, FALSE) AS attendee_reminders_enabled
                FROM calendar_events e
                LEFT JOIN calendar_automation_settings s
                  ON s.user_email = e.user_email
                LEFT JOIN calendar_reminder_deliveries d
                  ON d.user_email = e.user_email
                 AND d.provider = e.provider
                 AND d.event_id = e.event_id
                 AND d.event_start_time = e.start_time
                WHERE COALESCE(s.reminders_enabled, TRUE) = TRUE
                  AND d.event_id IS NULL
                  AND e.start_time >= (NOW() - INTERVAL '4 hours')
                  AND e.start_time <= (NOW() + INTERVAL '24 hours')
                  AND (
                      e.start_time - make_interval(mins => COALESCE(s.reminder_offset_minutes, 2))
                  ) <= NOW()
                ORDER BY e.start_time ASC
            """
            )

            reminders = []
            for row in rows:
                attendees_raw = row["attendee_emails"] or []
                attendees = (
                    json.loads(attendees_raw)
                    if isinstance(attendees_raw, str)
                    else attendees_raw
                )
                reminders.append(
                    {
                        "user_email": row["user_email"],
                        "provider": row["provider"],
                        "event_id": row["event_id"],
                        "meeting_title": row["meeting_title"],
                        "meeting_link": row["meeting_link"],
                        "attendees": attendees,
                        "start_time": row["start_time"],
                        "attendee_reminders_enabled": row[
                            "attendee_reminders_enabled"
                        ],
                    }
                )
            return reminders

    async def mark_calendar_reminder_sent(
        self,
        user_email: str,
        provider: str,
        event_id: str,
        event_start_time: datetime,
        recipients: List[str],
    ):
        async with self._get_connection() as conn:
            await conn.execute(
                """
                INSERT INTO calendar_reminder_deliveries (
                    user_email, provider, event_id, event_start_time, sent_at, recipients
                )
                VALUES ($1, $2, $3, $4, $5, $6::jsonb)
                ON CONFLICT (user_email, provider, event_id, event_start_time) DO NOTHING
            """,
                user_email,
                provider,
                event_id,
                event_start_time,
                datetime.utcnow(),
                json.dumps(recipients),
            )

    async def upsert_recording_session(
        self,
        session_id: str,
        user_email: str,
        meeting_id: str,
        status: str = "recording",
        metadata: Optional[Dict] = None,
    ) -> Dict:
        now = datetime.utcnow()
        async with self._get_connection() as conn:
            row = await conn.fetchrow(
                """
                INSERT INTO recording_sessions (
                    session_id, user_email, meeting_id, status, started_at,
                    last_heartbeat_at, metadata, created_at, updated_at
                )
                VALUES ($1, $2, $3, $4, $5, $5, $6::jsonb, $5, $5)
                ON CONFLICT (session_id) DO UPDATE SET
                    user_email = EXCLUDED.user_email,
                    meeting_id = EXCLUDED.meeting_id,
                    status = EXCLUDED.status,
                    last_heartbeat_at = EXCLUDED.last_heartbeat_at,
                    metadata = recording_sessions.metadata || EXCLUDED.metadata,
                    updated_at = EXCLUDED.updated_at
                RETURNING session_id, user_email, meeting_id, status, started_at, updated_at
            """,
                session_id,
                user_email,
                meeting_id,
                status,
                now,
                json.dumps(metadata or {}),
            )
            return dict(row) if row else {}

    async def touch_recording_session_heartbeat(self, session_id: str):
        async with self._get_connection() as conn:
            await conn.execute(
                """
                UPDATE recording_sessions
                SET last_heartbeat_at = $2, updated_at = $2
                WHERE session_id = $1
            """,
                session_id,
                datetime.utcnow(),
            )

    async def transition_recording_session_status(
        self,
        session_id: str,
        from_statuses: List[str],
        to_status: str,
        error_code: Optional[str] = None,
        error_message: Optional[str] = None,
    ) -> bool:
        now = datetime.utcnow()
        async with self._get_connection() as conn:
            result = await conn.execute(
                """
                UPDATE recording_sessions
                SET status = $3,
                    error_code = COALESCE($4, error_code),
                    error_message = COALESCE($5, error_message),
                    stop_requested_at = CASE
                        WHEN $3 = 'stopping_requested' THEN COALESCE(stop_requested_at, $6)
                        ELSE stop_requested_at
                    END,
                    stopped_at = CASE
                        WHEN $3 IN ('uploading_chunks', 'finalizing', 'postprocessing', 'completed', 'failed')
                        THEN COALESCE(stopped_at, $6)
                        ELSE stopped_at
                    END,
                    finalized_at = CASE
                        WHEN $3 = 'completed' THEN COALESCE(finalized_at, $6)
                        ELSE finalized_at
                    END,
                    updated_at = $6
                WHERE session_id = $1
                  AND status = ANY($2::text[])
            """,
                session_id,
                from_statuses,
                to_status,
                error_code,
                error_message,
                now,
            )
            return result != "UPDATE 0"

    async def get_recording_session(self, session_id: str) -> Optional[Dict]:
        async with self._get_connection() as conn:
            row = await conn.fetchrow(
                """
                SELECT session_id, user_email, meeting_id, status, started_at, stop_requested_at,
                       stopped_at, finalized_at, expected_chunk_count, finalized_chunk_count,
                       dropped_chunk_count, idempotency_finalize_key, last_heartbeat_at,
                       error_code, error_message, metadata, created_at, updated_at
                FROM recording_sessions
                WHERE session_id = $1
            """,
                session_id,
            )
            return dict(row) if row else None

    async def upsert_recording_chunk(
        self,
        session_id: str,
        chunk_index: int,
        byte_size: int,
        checksum: Optional[str] = None,
        storage_path: Optional[str] = None,
        upload_status: str = "pending",
    ):
        now = datetime.utcnow()
        async with self._get_connection() as conn:
            await conn.execute(
                """
                INSERT INTO recording_chunks (
                    session_id, chunk_index, byte_size, checksum, storage_path,
                    upload_status, created_at, updated_at
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $7)
                ON CONFLICT (session_id, chunk_index) DO UPDATE SET
                    byte_size = EXCLUDED.byte_size,
                    checksum = COALESCE(EXCLUDED.checksum, recording_chunks.checksum),
                    storage_path = COALESCE(EXCLUDED.storage_path, recording_chunks.storage_path),
                    upload_status = EXCLUDED.upload_status,
                    uploaded_at = CASE
                        WHEN EXCLUDED.upload_status = 'uploaded' THEN COALESCE(recording_chunks.uploaded_at, $7)
                        ELSE recording_chunks.uploaded_at
                    END,
                    updated_at = $7
            """,
                session_id,
                chunk_index,
                byte_size,
                checksum,
                storage_path,
                upload_status,
                now,
            )

    async def get_recording_chunk(self, session_id: str, chunk_index: int) -> Optional[Dict]:
        async with self._get_connection() as conn:
            row = await conn.fetchrow(
                """
                SELECT session_id, chunk_index, byte_size, checksum, storage_path,
                       upload_status, created_at, uploaded_at, updated_at
                FROM recording_chunks
                WHERE session_id = $1 AND chunk_index = $2
            """,
                session_id,
                chunk_index,
            )
            return dict(row) if row else None

    async def get_recording_chunk_stats(self, session_id: str) -> Dict:
        async with self._get_connection() as conn:
            row = await conn.fetchrow(
                """
                SELECT
                    COUNT(*)::int AS total,
                    COUNT(*) FILTER (WHERE upload_status = 'uploaded')::int AS uploaded,
                    COUNT(*) FILTER (WHERE upload_status = 'pending')::int AS pending,
                    COUNT(*) FILTER (WHERE upload_status = 'failed')::int AS failed
                FROM recording_chunks
                WHERE session_id = $1
            """,
                session_id,
            )
            return dict(row) if row else {
                "total": 0,
                "uploaded": 0,
                "pending": 0,
                "failed": 0,
            }

    async def update_recording_session_counters(
        self,
        session_id: str,
        expected_chunk_count: Optional[int] = None,
        finalized_chunk_count: Optional[int] = None,
        dropped_chunk_delta: int = 0,
    ):
        async with self._get_connection() as conn:
            await conn.execute(
                """
                UPDATE recording_sessions
                SET expected_chunk_count = COALESCE($2, expected_chunk_count),
                    finalized_chunk_count = COALESCE($3, finalized_chunk_count),
                    dropped_chunk_count = dropped_chunk_count + $4,
                    updated_at = $5
                WHERE session_id = $1
            """,
                session_id,
                expected_chunk_count,
                finalized_chunk_count,
                dropped_chunk_delta,
                datetime.utcnow(),
            )

    async def get_stale_recording_sessions(
        self, statuses: List[str], stale_after_minutes: int = 10, limit: int = 100
    ) -> List[Dict]:
        async with self._get_connection() as conn:
            rows = await conn.fetch(
                """
                SELECT session_id, user_email, meeting_id, status, started_at, stop_requested_at,
                       stopped_at, finalized_at, expected_chunk_count, finalized_chunk_count,
                       dropped_chunk_count, idempotency_finalize_key, last_heartbeat_at,
                       error_code, error_message, metadata, created_at, updated_at
                FROM recording_sessions
                WHERE status = ANY($1::text[])
                  AND updated_at <= (NOW() - make_interval(mins => $2))
                ORDER BY updated_at ASC
                LIMIT $3
            """,
                statuses,
                stale_after_minutes,
                limit,
            )
            return [dict(r) for r in rows]

    async def set_recording_finalize_key(self, session_id: str, finalize_key: str):
        async with self._get_connection() as conn:
            await conn.execute(
                """
                UPDATE recording_sessions
                SET idempotency_finalize_key = $2, updated_at = $3
                WHERE session_id = $1
            """,
                session_id,
                finalize_key,
                datetime.utcnow(),
            )

    async def merge_recording_session_metadata(
        self, session_id: str, metadata_patch: Dict
    ):
        async with self._get_connection() as conn:
            await conn.execute(
                """
                UPDATE recording_sessions
                SET metadata = COALESCE(metadata, '{}'::jsonb) || $2::jsonb,
                    updated_at = $3
                WHERE session_id = $1
            """,
                session_id,
                json.dumps(metadata_patch or {}),
                datetime.utcnow(),
            )

    async def search_transcripts(self, query: str):
        """Search through meeting transcripts for the given query"""
        if not query or query.strip() == "":
            return []

        search_query = f"%{query.lower()}%"

        try:
            async with self._get_connection() as conn:
                # 1. Search transcript_segments table
                rows = await conn.fetch(
                    """
                    SELECT m.id, m.title, ts.transcript, ts.timestamp
                    FROM meetings m
                    JOIN transcript_segments ts ON m.id = ts.meeting_id
                    WHERE LOWER(ts.transcript) LIKE $1
                    ORDER BY m.created_at DESC
                """,
                    search_query,
                )

                # 2. Search full_transcripts table
                chunk_rows = await conn.fetch(
                    """
                    SELECT m.id, m.title, ft.transcript_text
                    FROM meetings m
                    JOIN full_transcripts ft ON m.id = ft.meeting_id
                    WHERE LOWER(ft.transcript_text) LIKE $1
                    AND m.id NOT IN (SELECT DISTINCT meeting_id FROM transcript_segments WHERE LOWER(transcript) LIKE $2)
                    ORDER BY m.created_at DESC
                """,
                    search_query,
                    search_query,
                )

                results = []

                # Helper to format results
                def format_match(row, text_col):
                    text = row[text_col]
                    lower_text = text.lower()
                    match_idx = lower_text.find(query.lower())
                    start = max(0, match_idx - 100)
                    end = min(len(text), match_idx + len(query) + 100)
                    context = text[start:end]
                    if start > 0:
                        context = "..." + context
                    if end < len(text):
                        context += "..."

                    return {
                        "id": row["id"],
                        "title": row["title"],
                        "matchContext": context,
                        "timestamp": row.get("timestamp")
                        or datetime.utcnow().isoformat(),
                    }

                for row in rows:
                    results.append(format_match(row, "transcript"))

                for row in chunk_rows:
                    results.append(format_match(row, "transcript_text"))

                return results

        except Exception as e:
            logger.error(f"Error searching transcripts: {str(e)}")
            raise

    async def delete_api_key(self, provider: str):
        """Delete the API key"""
        provider_map = {
            "openai": "openaiapikey",
            "claude": "anthropicapikey",
            "groq": "groqapikey",
            "ollama": "ollamaapikey",
            "gemini": "geminiapikey",
        }
        if provider not in provider_map:
            raise ValueError(f"Invalid provider: {provider}")

        column_name = provider_map[provider]
        async with self._get_connection() as conn:
            await conn.execute(
                f"UPDATE settings SET \"{column_name}\" = NULL WHERE id = '1'"
            )

    async def create_feedback(
        self,
        feedback_id: str,
        user_id: str,
        user_email: str,
        type: str,
        title: str,
        description: str,
    ):
        """Create new feedback entry"""
        async with self._get_connection() as conn:
            await conn.execute(
                """
                INSERT INTO feedback (id, user_id, user_email, type, title, description)
                VALUES ($1, $2, $3, $4, $5, $6)
            """,
                feedback_id,
                user_id,
                user_email,
                type,
                title,
                description,
            )

    async def get_feedback(self, user_id: Optional[str] = None):
        """Get all feedback or filter by user_id"""
        async with self._get_connection() as conn:
            if user_id:
                rows = await conn.fetch(
                    "SELECT * FROM feedback WHERE user_id = $1 ORDER BY created_at DESC",
                    user_id,
                )
            else:
                rows = await conn.fetch(
                    "SELECT * FROM feedback ORDER BY created_at DESC"
                )

            # Format datetime objects to strings
            results = []
            for row in rows:
                item = dict(row)
                item["created_at"] = (
                    item["created_at"].isoformat() if item["created_at"] else None
                )
                item["updated_at"] = (
                    item["updated_at"].isoformat() if item["updated_at"] else None
                )
                results.append(item)
            return results

    async def update_feedback_status(self, feedback_id: str, status: str):
        """Update status of a feedback item"""
        now = datetime.utcnow()
        async with self._get_connection() as conn:
            await conn.execute(
                """
                UPDATE feedback 
                SET status = $1, updated_at = $2 
                WHERE id = $3
            """,
                status,
                now,
                feedback_id,
            )
