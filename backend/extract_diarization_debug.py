
import asyncio
import os
import json
import asyncpg
from datetime import datetime

async def extract_data(meeting_id):
    db_url = os.getenv("DATABASE_URL")
    if not db_url:
        print("DATABASE_URL not set")
        return

    conn = await asyncpg.connect(db_url)
    try:
        # 1. Get Deepgram Segments from diarization_jobs
        job_row = await conn.fetchrow(
            "SELECT result_json, provider FROM diarization_jobs WHERE meeting_id = $1",
            meeting_id
        )
        
        # 2. Get Aligned Segments from transcript_versions
        version_row = await conn.fetchrow(
            """
            SELECT content_json, alignment_config 
            FROM transcript_versions 
            WHERE meeting_id = $1 AND source = 'diarized'
            ORDER BY version_num DESC LIMIT 1
            """,
            meeting_id
        )

        # 3. Get Live Transcripts (as a proxy for Groq baseline if we don't have it saved)
        live_rows = await conn.fetch(
            """
            SELECT transcript, audio_start_time, audio_end_time 
            FROM transcript_segments 
            WHERE meeting_id = $1 AND (source = 'live' OR source = 'web_client')
            ORDER BY audio_start_time ASC
            """,
            meeting_id
        )

        data = {
            "deepgram": json.loads(job_row['result_json']) if job_row and job_row['result_json'] else None,
            "aligned": json.loads(version_row['content_json']) if version_row and version_row['content_json'] else None,
            "alignment_config": json.loads(version_row['alignment_config']) if version_row and version_row['alignment_config'] else None,
            "live": [dict(r) for r in live_rows]
        }

        with open("diarization_debug_output.json", "w") as f:
            json.dump(data, f, indent=2, default=str)
        print("Data extracted to diarization_debug_output.json")

    finally:
        await conn.close()

if __name__ == "__main__":
    meeting_id = "c6345fa5-246a-4a36-9c26-2c95cb074310"
    asyncio.run(extract_data(meeting_id))
