import asyncio
import os
import json
import logging
import sys
from datetime import datetime
from pathlib import Path

# Add app to path
sys.path.append(os.getcwd())

from app.db import DatabaseManager

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

async def debug_meeting(meeting_id: str):
    db_url = os.getenv("DATABASE_URL")
    db = DatabaseManager(db_url)
    
    logger.info(f"🔍 Debugging meeting {meeting_id}")
    
    async with db._get_connection() as conn:
        # 1. Fetch Diarization Job Results
        job = await conn.fetchrow(
            "SELECT status, result_json, provider FROM diarization_jobs WHERE meeting_id = $1", 
            meeting_id
        )
        if not job:
            logger.error("Diarization job not found")
            return
            
        logger.info(f"Job Status: {job['status']}")
        if job['result_json']:
            diarization_segments = json.loads(job['result_json'])
            logger.info(f"raw Diarization segments (Deepgram) - Found {len(diarization_segments)}:")
            for s in diarization_segments[:20]:
                print(f"[{s.get('start', '??'):.2f} - {s.get('end', '??'):.2f}] {s.get('speaker', 'Unknown')}")
        else:
            logger.error("No diarization results found in job")

        # 2. Fetch Latest Diarized Transcript Version
        version = await conn.fetchrow(
            "SELECT content_json, alignment_config FROM transcript_versions WHERE meeting_id = $1 AND source = 'diarized' ORDER BY version_num DESC LIMIT 1",
            meeting_id
        )
        
        if version:
            final_segments = json.loads(version['content_json'])
            logger.info(f"Found {len(final_segments)} final aligned segments")
            
            logger.info("First 10 final segments:")
            for s in final_segments[:10]:
                print(f"[{s.get('start', '??')}] {s.get('speaker', 'Unknown')} (Conf: {s.get('speaker_confidence', 0):.2f}, State: {s.get('alignment_state', 'N/A')}): {s.get('text', '')[:60]}...")
        else:
            logger.error("No diarized transcript version found")

    # Fetch 'live' or 'final' segments for comparison
    async with db._get_connection() as conn:
        logger.info("\n--- Live/Final Segments (Groq/Baseline) ---")
        rows = await conn.fetch(
            "SELECT transcript, audio_start_time, audio_end_time FROM transcript_segments WHERE meeting_id = $1 AND (source = 'live' OR source = 'final') ORDER BY audio_start_time",
            meeting_id
        )
        logger.info(f"Found {len(rows)} live/final segments")
        for r in rows[:15]:
            print(f"[{r['audio_start_time']}] {r['transcript'][:80]}")

        logger.info("\n--- Diarized Segments in DB table ---")
        rows = await conn.fetch(
            "SELECT transcript, speaker, audio_start_time, audio_end_time, alignment_state, speaker_confidence FROM transcript_segments WHERE meeting_id = $1 AND source = 'diarized' ORDER BY audio_start_time",
            meeting_id
        )
        for r in rows[:15]:
            start = r['audio_start_time'] if r['audio_start_time'] is not None else 0.0
            print(f"[{start:.2f}] {r['speaker']} ({r['speaker_confidence']:.2f}, {r['alignment_state']}): {r['transcript'][:80]}")

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: python debug_groq_and_alignment.py <meeting_id>")
        sys.exit(1)
    
    meeting_id = sys.argv[1]
    asyncio.run(debug_meeting(meeting_id))
