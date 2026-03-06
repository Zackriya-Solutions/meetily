import asyncio
import time
import os
import sys

# Add the project root to sys.path
sys.path.append(os.path.abspath(os.path.join(os.path.dirname(__file__), "..")))

from app.services.ai_participant import AIParticipantEngine, MeetingContext

class MockDB:
    async def get_api_key(self, provider, user_email):
        return "mock_key"

async def test_buffer_logic():
    print("Testing AI Participant Buffer Logic...")
    
    # Set env var for testing
    os.environ["AI_PARTICIPANT_WINDOW_SECONDS"] = "300" # 5 mins
    
    context = MeetingContext(meeting_id="test_meeting", goal="Test Goal")
    engine = AIParticipantEngine(db=MockDB(), user_email="test@appointy.com", meeting_context=context)
    
    # Simulate transcript ingestion over time
    start_time = time.time()
    
    # Add some text at T=0
    engine.buffer.add(start_time, "Meeting started.")
    print(f"T=0: Buffer duration: {engine.buffer.get_duration_seconds()}s, Text: {engine.buffer.get_text()}")
    
    # Add text at T=60
    engine.buffer.add(start_time + 60, "Discussing agenda.")
    print(f"T=60: Buffer duration: {engine.buffer.get_duration_seconds()}s")
    
    # Add text at T=301 (should prune the T=0 item)
    engine.buffer.add(start_time + 301, "Moving to next topic.")
    print(f"T=301: Buffer duration: {engine.buffer.get_duration_seconds()}s")
    
    text = engine.buffer.get_text()
    print(f"Final Buffer Text:\n{text}")
    
    if "Meeting started." not in text and "Discussing agenda." in text:
        print("✅ Buffer correctly pruned old items.")
    else:
        print("❌ Buffer pruning failed or timing mismatch.")

    # Check window duration pick up
    print(f"Engine Window Duration Config: {engine.buffer.window_seconds}s")
    if engine.buffer.window_seconds == 300:
        print("✅ Engine correctly picked up AI_PARTICIPANT_WINDOW_SECONDS=300")
    else:
        print(f"❌ Engine picked up {engine.buffer.window_seconds} instead of 300")

if __name__ == "__main__":
    asyncio.run(test_buffer_logic())
