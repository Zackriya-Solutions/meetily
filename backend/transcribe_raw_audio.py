import asyncio
import os
import json
import logging
import sys
from pathlib import Path
import httpx

# Add app to path
sys.path.append(os.getcwd())

from app.services.file_processing import download_audio
from app.services.audio.groq_client import GroqTranscriptionClient
from app.services.audio.diarization import DiarizationService

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

async def transcribe_raw(meeting_id: str):
    logger.info(f"📥 Downloading audio for {meeting_id}...")
    try:
        audio_data = await download_audio(meeting_id)
        if not audio_data:
            logger.error("Failed to download audio data")
            return
        logger.info(f"✅ Downloaded {len(audio_data)} bytes")
    except Exception as e:
        logger.error(f"Download error: {e}")
        return

    # Normalise to WAV 16k mono (what diarization uses)
    ds = DiarizationService()
    logger.info("📦 Normalizing audio to 16k mono WAV...")
    wav_data = await ds.ensure_wav_audio(audio_data, meeting_id)
    logger.info(f"✅ WAV size: {len(wav_data)} bytes")

    # Transcribe with Groq
    groq_api_key = os.getenv("GROQ_API_KEY")
    groq = GroqTranscriptionClient(groq_api_key)
    
    logger.info("🚀 Sending to Groq Whisper (raw full file)...")
    segments = await groq.transcribe_audio(wav_data)
    
    logger.info(f"✅ Received {len(segments)} segments from Groq")
    
    print("\n--- RAW GROQ TRANSCRIPT ---")
    full_text = []
    for s in segments:
        print(f"[{s.start:.2f} - {s.end:.2f}] {s.text}")
        full_text.append(s.text)
    
    # Save to file
    output_path = f"raw_transcript_{meeting_id}.txt"
    with open(output_path, "w") as f:
        f.write("\n".join([f"[{s.start:.2f}] {s.text}" for s in segments]))
    logger.info(f"✅ Full transcript saved to {output_path}")

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: python transcribe_raw_audio.py <meeting_id>")
        sys.exit(1)
    
    meeting_id = sys.argv[1]
    asyncio.run(transcribe_raw(meeting_id))
