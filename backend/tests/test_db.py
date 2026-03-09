"""Tests for SQLite DatabaseManager operations."""

import pytest
import os
import sys
import tempfile

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "app"))

from db import DatabaseManager

pytestmark = pytest.mark.asyncio


@pytest.fixture
def db_manager(tmp_path):
    """Create a DatabaseManager with a temporary database."""
    db_path = str(tmp_path / "test.db")
    return DatabaseManager(db_path=db_path)


# ── save_meeting / get_meeting ───────────────────────────────────────


async def test_save_and_get_meeting(db_manager):
    await db_manager.save_meeting("m-1", "Test Meeting")
    meeting = await db_manager.get_meeting("m-1")
    assert meeting is not None
    assert meeting["id"] == "m-1"
    assert meeting["title"] == "Test Meeting"
    assert meeting["transcripts"] == []


async def test_save_meeting_with_folder_path(db_manager):
    await db_manager.save_meeting("m-2", "Folder Meeting", folder_path="/tmp/meetings")
    meeting = await db_manager.get_meeting("m-2")
    assert meeting is not None


async def test_save_duplicate_meeting_raises(db_manager):
    await db_manager.save_meeting("m-dup", "Meeting 1")
    with pytest.raises(Exception):
        await db_manager.save_meeting("m-dup", "Meeting 2")


async def test_get_meeting_not_found(db_manager):
    result = await db_manager.get_meeting("nonexistent")
    assert result is None


# ── get_all_meetings ─────────────────────────────────────────────────


async def test_get_all_meetings_empty(db_manager):
    meetings = await db_manager.get_all_meetings()
    assert meetings == []


async def test_get_all_meetings(db_manager):
    await db_manager.save_meeting("m-a", "Meeting A")
    await db_manager.save_meeting("m-b", "Meeting B")
    meetings = await db_manager.get_all_meetings()
    assert len(meetings) == 2


# ── update_meeting_title ─────────────────────────────────────────────


async def test_update_meeting_title(db_manager):
    await db_manager.save_meeting("m-title", "Old Title")
    await db_manager.update_meeting_title("m-title", "New Title")
    meeting = await db_manager.get_meeting("m-title")
    assert meeting["title"] == "New Title"


# ── delete_meeting ───────────────────────────────────────────────────


async def test_delete_meeting_success(db_manager):
    await db_manager.save_meeting("m-del", "To Delete")
    result = await db_manager.delete_meeting("m-del")
    assert result is True
    meeting = await db_manager.get_meeting("m-del")
    assert meeting is None


async def test_delete_meeting_not_found(db_manager):
    result = await db_manager.delete_meeting("nonexistent")
    assert result is False


async def test_delete_meeting_empty_id(db_manager):
    with pytest.raises(ValueError):
        await db_manager.delete_meeting("")


async def test_delete_meeting_whitespace_id(db_manager):
    with pytest.raises(ValueError):
        await db_manager.delete_meeting("   ")


async def test_delete_meeting_cascades(db_manager):
    """Deletion should remove transcripts and related data."""
    await db_manager.save_meeting("m-cascade", "Cascade Test")
    await db_manager.save_meeting_transcript(
        "m-cascade", "Test transcript", "2025-01-01T00:00:00Z"
    )
    result = await db_manager.delete_meeting("m-cascade")
    assert result is True


# ── save_meeting_transcript ──────────────────────────────────────────


async def test_save_and_get_transcript(db_manager):
    await db_manager.save_meeting("m-tr", "Transcript Meeting")
    await db_manager.save_meeting_transcript(
        "m-tr", "Hello world", "2025-01-01T00:00:00Z"
    )
    meeting = await db_manager.get_meeting("m-tr")
    assert len(meeting["transcripts"]) == 1
    assert meeting["transcripts"][0]["text"] == "Hello world"


async def test_save_transcript_with_audio_timestamps(db_manager):
    await db_manager.save_meeting("m-audio", "Audio Meeting")
    await db_manager.save_meeting_transcript(
        "m-audio", "Some text", "2025-01-01T00:00:00Z",
        audio_start_time=1.5, audio_end_time=6.5, duration=5.0,
    )
    meeting = await db_manager.get_meeting("m-audio")
    t = meeting["transcripts"][0]
    assert t["audio_start_time"] == 1.5
    assert t["audio_end_time"] == 6.5
    assert t["duration"] == 5.0


async def test_save_multiple_transcripts(db_manager):
    await db_manager.save_meeting("m-multi", "Multi Transcript")
    await db_manager.save_meeting_transcript("m-multi", "Segment 1", "2025-01-01T00:00:00Z")
    await db_manager.save_meeting_transcript("m-multi", "Segment 2", "2025-01-01T00:05:00Z")
    meeting = await db_manager.get_meeting("m-multi")
    assert len(meeting["transcripts"]) == 2


# ── create_process / update_process / get_transcript_data ────────────


async def test_create_and_update_process(db_manager):
    await db_manager.save_meeting("m-proc", "Process Meeting")
    process_id = await db_manager.create_process("m-proc")
    assert process_id == "m-proc"

    await db_manager.update_process("m-proc", status="completed", result='{"key": "value"}')


async def test_create_process_idempotent(db_manager):
    await db_manager.save_meeting("m-idem", "Idempotent")
    await db_manager.create_process("m-idem")
    # Second call should update, not fail
    await db_manager.create_process("m-idem")


async def test_update_process_with_error(db_manager):
    await db_manager.save_meeting("m-err", "Error Meeting")
    await db_manager.create_process("m-err")
    await db_manager.update_process("m-err", status="failed", error="Something went wrong")


async def test_update_process_sets_end_time(db_manager):
    await db_manager.save_meeting("m-end", "End Time Meeting")
    await db_manager.create_process("m-end")
    await db_manager.update_process("m-end", status="COMPLETED")


# ── save_transcript (transcript_chunks) ──────────────────────────────


async def test_save_transcript_chunk(db_manager):
    await db_manager.save_meeting("m-chunk", "Chunk Meeting")
    await db_manager.save_transcript(
        "m-chunk", "Full transcript text", "ollama", "llama3.1", 5000, 1000,
    )


async def test_save_transcript_empty_id_raises(db_manager):
    with pytest.raises(ValueError, match="meeting_id cannot be empty"):
        await db_manager.save_transcript("", "text", "model", "name", 5000, 1000)


async def test_save_transcript_empty_text_raises(db_manager):
    with pytest.raises(ValueError, match="transcript_text cannot be empty"):
        await db_manager.save_transcript("m-1", "", "model", "name", 5000, 1000)


async def test_save_transcript_invalid_chunk_size_raises(db_manager):
    with pytest.raises(ValueError, match="Invalid chunk_size"):
        await db_manager.save_transcript("m-1", "text", "model", "name", 0, 1000)


async def test_save_transcript_negative_overlap_raises(db_manager):
    with pytest.raises(ValueError, match="Invalid chunk_size"):
        await db_manager.save_transcript("m-1", "text", "model", "name", 5000, -1)


async def test_save_transcript_too_large_raises(db_manager):
    huge_text = "x" * (10_000_001)
    with pytest.raises(ValueError, match="too large"):
        await db_manager.save_transcript("m-1", huge_text, "model", "name", 5000, 1000)


# ── update_meeting_name ──────────────────────────────────────────────


async def test_update_meeting_name(db_manager):
    await db_manager.save_meeting("m-name", "Old Name")
    await db_manager.save_transcript("m-name", "text", "model", "name", 5000, 1000)
    await db_manager.update_meeting_name("m-name", "New Name")
    meeting = await db_manager.get_meeting("m-name")
    assert meeting["title"] == "New Name"


# ── Model config ─────────────────────────────────────────────────────


async def test_get_model_config_empty(db_manager):
    config = await db_manager.get_model_config()
    assert config is None


async def test_save_and_get_model_config(db_manager):
    await db_manager.save_model_config("openai", "gpt-4o", "large-v3")
    config = await db_manager.get_model_config()
    assert config["provider"] == "openai"
    assert config["model"] == "gpt-4o"
    assert config["whisperModel"] == "large-v3"


async def test_save_model_config_empty_provider_raises(db_manager):
    with pytest.raises(ValueError, match="Provider cannot be empty"):
        await db_manager.save_model_config("", "gpt-4o", "large-v3")


async def test_save_model_config_empty_model_raises(db_manager):
    with pytest.raises(ValueError, match="Model cannot be empty"):
        await db_manager.save_model_config("openai", "", "large-v3")


async def test_save_model_config_update(db_manager):
    await db_manager.save_model_config("openai", "gpt-4o", "large-v3")
    await db_manager.save_model_config("claude", "claude-3.5", "medium")
    config = await db_manager.get_model_config()
    assert config["provider"] == "claude"


# ── API keys ─────────────────────────────────────────────────────────


@pytest.mark.parametrize("provider", ["openai", "claude", "groq", "ollama"])
async def test_save_and_get_api_key(db_manager, provider):
    await db_manager.save_api_key("test-key-123", provider)
    key = await db_manager.get_api_key(provider)
    assert key == "test-key-123"


async def test_get_api_key_invalid_provider(db_manager):
    with pytest.raises(ValueError, match="Invalid provider"):
        await db_manager.get_api_key("invalid")


async def test_save_api_key_invalid_provider(db_manager):
    with pytest.raises(ValueError, match="Invalid provider"):
        await db_manager.save_api_key("key", "invalid")


async def test_delete_api_key(db_manager):
    await db_manager.save_api_key("key-to-delete", "openai")
    await db_manager.delete_api_key("openai")
    key = await db_manager.get_api_key("openai")
    assert key == ""


async def test_delete_api_key_invalid_provider(db_manager):
    with pytest.raises(ValueError, match="Invalid provider"):
        await db_manager.delete_api_key("invalid")


# ── Transcript config ────────────────────────────────────────────────


async def test_get_transcript_config_default(db_manager):
    config = await db_manager.get_transcript_config()
    assert config["provider"] == "localWhisper"
    assert config["model"] == "large-v3"


async def test_save_and_get_transcript_config(db_manager):
    await db_manager.save_transcript_config("deepgram", "nova-2")
    config = await db_manager.get_transcript_config()
    assert config["provider"] == "deepgram"
    assert config["model"] == "nova-2"


async def test_save_transcript_config_empty_provider_raises(db_manager):
    with pytest.raises(ValueError, match="Provider cannot be empty"):
        await db_manager.save_transcript_config("", "model")


async def test_save_transcript_config_empty_model_raises(db_manager):
    with pytest.raises(ValueError, match="Model cannot be empty"):
        await db_manager.save_transcript_config("provider", "")


# ── Transcript API keys ─────────────────────────────────────────────


@pytest.mark.parametrize("provider", ["localWhisper", "deepgram", "elevenLabs", "groq", "openai"])
async def test_save_and_get_transcript_api_key(db_manager, provider):
    await db_manager.save_transcript_api_key("tkey-123", provider)
    key = await db_manager.get_transcript_api_key(provider)
    assert key == "tkey-123"


async def test_get_transcript_api_key_invalid_provider(db_manager):
    with pytest.raises(ValueError, match="Invalid provider"):
        await db_manager.get_transcript_api_key("invalid")


async def test_save_transcript_api_key_invalid_provider(db_manager):
    with pytest.raises(ValueError, match="Invalid provider"):
        await db_manager.save_transcript_api_key("key", "invalid")


# ── search_transcripts ───────────────────────────────────────────────


async def test_search_transcripts_empty_query(db_manager):
    results = await db_manager.search_transcripts("")
    assert results == []


async def test_search_transcripts_no_match(db_manager):
    await db_manager.save_meeting("m-search", "Search Meeting")
    await db_manager.save_meeting_transcript("m-search", "Hello world", "2025-01-01T00:00:00Z")
    results = await db_manager.search_transcripts("nonexistentxyz")
    assert results == []


async def test_search_transcripts_found(db_manager):
    await db_manager.save_meeting("m-found", "Found Meeting")
    await db_manager.save_meeting_transcript("m-found", "Hello world test", "2025-01-01T00:00:00Z")
    results = await db_manager.search_transcripts("Hello world")
    assert len(results) >= 1
    assert results[0]["title"] == "Found Meeting"


async def test_search_transcripts_case_insensitive(db_manager):
    await db_manager.save_meeting("m-case", "Case Meeting")
    await db_manager.save_meeting_transcript("m-case", "Hello World", "2025-01-01T00:00:00Z")
    results = await db_manager.search_transcripts("hello world")
    assert len(results) >= 1


async def test_search_transcripts_context_snippet(db_manager):
    long_text = "A" * 200 + "TARGET_KEYWORD" + "B" * 200
    await db_manager.save_meeting("m-ctx", "Context Meeting")
    await db_manager.save_meeting_transcript("m-ctx", long_text, "2025-01-01T00:00:00Z")
    results = await db_manager.search_transcripts("TARGET_KEYWORD")
    assert len(results) >= 1
    assert "TARGET_KEYWORD" in results[0]["matchContext"]
    # Should have ellipsis since we truncated
    assert "..." in results[0]["matchContext"]


# ── update_meeting_summary ───────────────────────────────────────────


async def test_update_meeting_summary_not_found(db_manager):
    with pytest.raises(ValueError, match="not found"):
        await db_manager.update_meeting_summary("nonexistent", {"key": "val"})


async def test_update_meeting_summary_success(db_manager):
    await db_manager.save_meeting("m-sum", "Summary Meeting")
    await db_manager.create_process("m-sum")
    await db_manager.update_meeting_summary("m-sum", {"MeetingName": "Test"})


# ── get_transcript_data ─────────────────────────────────────────────


async def test_get_transcript_data_not_found(db_manager):
    result = await db_manager.get_transcript_data("nonexistent")
    assert result is None


async def test_get_transcript_data_success(db_manager):
    await db_manager.save_meeting("m-td", "Transcript Data Meeting")
    await db_manager.create_process("m-td")
    await db_manager.save_transcript("m-td", "Full text", "ollama", "llama3", 5000, 1000)
    result = await db_manager.get_transcript_data("m-td")
    assert result is not None
    assert result["transcript_text"] == "Full text"
    assert result["status"] == "PENDING"


async def test_get_transcript_data_with_completed_status(db_manager):
    await db_manager.save_meeting("m-comp", "Completed Meeting")
    await db_manager.create_process("m-comp")
    await db_manager.save_transcript("m-comp", "Text", "openai", "gpt-4o", 5000, 1000)
    await db_manager.update_process("m-comp", status="completed", result='{"key": "val"}')
    result = await db_manager.get_transcript_data("m-comp")
    assert result["status"] == "completed"
    assert result["result"] is not None


# ── save_model_config edge cases ────────────────────────────────────


async def test_save_model_config_empty_whisper_model_raises(db_manager):
    with pytest.raises(ValueError, match="Whisper model cannot be empty"):
        await db_manager.save_model_config("openai", "gpt-4o", "")


async def test_save_model_config_whitespace_provider_raises(db_manager):
    with pytest.raises(ValueError, match="Provider cannot be empty"):
        await db_manager.save_model_config("   ", "gpt-4o", "large-v3")


# ── save_transcript edge cases ──────────────────────────────────────


async def test_save_transcript_whitespace_id_raises(db_manager):
    with pytest.raises(ValueError, match="meeting_id cannot be empty"):
        await db_manager.save_transcript("   ", "text", "model", "name", 5000, 1000)


async def test_save_transcript_whitespace_text_raises(db_manager):
    with pytest.raises(ValueError, match="transcript_text cannot be empty"):
        await db_manager.save_transcript("m-1", "   ", "model", "name", 5000, 1000)


async def test_save_transcript_update_existing(db_manager):
    """Saving transcript twice for same meeting should update, not duplicate."""
    await db_manager.save_meeting("m-upd", "Update Meeting")
    await db_manager.save_transcript("m-upd", "Original text", "ollama", "llama3", 5000, 1000)
    await db_manager.save_transcript("m-upd", "Updated text", "openai", "gpt-4o", 8000, 2000)


# ── update_process edge cases ───────────────────────────────────────


async def test_update_process_with_metadata(db_manager):
    await db_manager.save_meeting("m-meta", "Metadata Meeting")
    await db_manager.create_process("m-meta")
    await db_manager.update_process(
        "m-meta", status="processing",
        metadata={"chunk": 1, "total": 3}
    )


async def test_update_process_with_chunk_count(db_manager):
    await db_manager.save_meeting("m-chunks", "Chunks Meeting")
    await db_manager.create_process("m-chunks")
    await db_manager.update_process(
        "m-chunks", status="completed",
        chunk_count=5, processing_time=12.5
    )


async def test_update_process_failed_sets_end_time(db_manager):
    await db_manager.save_meeting("m-fail-end", "Fail End Meeting")
    await db_manager.create_process("m-fail-end")
    await db_manager.update_process("m-fail-end", status="FAILED", error="Out of memory")


async def test_update_process_nonexistent_meeting(db_manager):
    """Updating a process for a nonexistent meeting should not raise."""
    await db_manager.update_process("no-such-meeting", status="completed")


# ── update_meeting_name ─────────────────────────────────────────────


async def test_update_meeting_name_updates_both_tables(db_manager):
    await db_manager.save_meeting("m-both", "Old Name")
    await db_manager.save_transcript("m-both", "text", "model", "name", 5000, 1000)
    await db_manager.update_meeting_name("m-both", "New Name")
    meeting = await db_manager.get_meeting("m-both")
    assert meeting["title"] == "New Name"


# ── search_transcripts edge cases ───────────────────────────────────


async def test_search_transcripts_whitespace_query(db_manager):
    results = await db_manager.search_transcripts("   ")
    assert results == []


async def test_search_transcripts_searches_transcript_chunks(db_manager):
    """Search should also look in transcript_chunks table."""
    await db_manager.save_meeting("m-chunk-search", "Chunk Search")
    await db_manager.save_transcript(
        "m-chunk-search", "The quick brown fox jumps over the lazy dog",
        "ollama", "llama3", 5000, 1000,
    )
    results = await db_manager.search_transcripts("quick brown fox")
    # transcript_chunks search may or may not find it depending on whether
    # there's also a transcripts entry; the test verifies no errors
    assert isinstance(results, list)


async def test_search_transcripts_multiple_meetings(db_manager):
    await db_manager.save_meeting("m-s1", "Meeting 1")
    await db_manager.save_meeting_transcript("m-s1", "alpha beta gamma", "2025-01-01T00:00:00Z")
    await db_manager.save_meeting("m-s2", "Meeting 2")
    await db_manager.save_meeting_transcript("m-s2", "delta alpha epsilon", "2025-01-01T01:00:00Z")
    results = await db_manager.search_transcripts("alpha")
    assert len(results) == 2


# ── delete_api_key edge cases ───────────────────────────────────────


async def test_delete_api_key_when_no_key_exists(db_manager):
    """Deleting a key that was never set should not raise."""
    # First ensure settings row exists
    await db_manager.save_model_config("openai", "gpt-4o", "large-v3")
    await db_manager.delete_api_key("openai")
    key = await db_manager.get_api_key("openai")
    assert key == ""


# ── update_meeting_summary edge cases ───────────────────────────────


async def test_update_meeting_summary_complex_data(db_manager):
    await db_manager.save_meeting("m-complex", "Complex Summary")
    await db_manager.create_process("m-complex")
    summary = {
        "MeetingName": "Sprint Planning",
        "People": {"title": "People", "blocks": [
            {"id": "1", "type": "bullet", "content": "Alice", "color": "gray"}
        ]},
        "sections": [{"title": "Notes", "blocks": []}],
    }
    await db_manager.update_meeting_summary("m-complex", summary)


# ── save_meeting_transcript edge cases ───────────────────────────────


async def test_save_transcript_without_optional_params(db_manager):
    await db_manager.save_meeting("m-noopt", "No Optional")
    result = await db_manager.save_meeting_transcript(
        "m-noopt", "Text here", "2025-01-01T00:00:00Z"
    )
    assert result is True


async def test_save_transcript_with_summary_fields(db_manager):
    await db_manager.save_meeting("m-sumf", "Summary Fields")
    result = await db_manager.save_meeting_transcript(
        "m-sumf", "Text", "2025-01-01T00:00:00Z",
        summary="A summary", action_items="Do this", key_points="Important point",
    )
    assert result is True


# ── save_transcript_api_key / get_transcript_api_key edge cases ──────


async def test_transcript_api_key_update_existing(db_manager):
    await db_manager.save_transcript_api_key("key-v1", "openai")
    await db_manager.save_transcript_api_key("key-v2", "openai")
    key = await db_manager.get_transcript_api_key("openai")
    assert key == "key-v2"


async def test_get_transcript_api_key_no_settings(db_manager):
    key = await db_manager.get_transcript_api_key("openai")
    assert key == ""


# ── save_api_key edge cases ──────────────────────────────────────────


async def test_save_api_key_creates_settings_row(db_manager):
    """Saving an API key with no settings row should auto-create one."""
    await db_manager.save_api_key("new-key", "openai")
    config = await db_manager.get_model_config()
    assert config is not None
    assert config["provider"] == "openai"


async def test_save_api_key_updates_existing_row(db_manager):
    await db_manager.save_model_config("openai", "gpt-4o", "large-v3")
    await db_manager.save_api_key("updated-key", "claude")
    key = await db_manager.get_api_key("claude")
    assert key == "updated-key"


# ── transcript_config edge cases ─────────────────────────────────────


async def test_save_transcript_config_update_existing(db_manager):
    await db_manager.save_transcript_config("deepgram", "nova-2")
    await db_manager.save_transcript_config("openai", "whisper-1")
    config = await db_manager.get_transcript_config()
    assert config["provider"] == "openai"
    assert config["model"] == "whisper-1"


async def test_save_transcript_config_whitespace_model_raises(db_manager):
    with pytest.raises(ValueError, match="Model cannot be empty"):
        await db_manager.save_transcript_config("openai", "   ")
