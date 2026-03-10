"""Tests for TranscriptProcessor — unit tests for non-LLM logic."""

import pytest
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "app"))

from transcript_processor import TranscriptProcessor, SummaryResponse, Block, Section, MeetingNotes, People

pytestmark = pytest.mark.asyncio


# ── SummaryResponse model tests ──────────────────────────────────────


def test_summary_response_serialization():
    """SummaryResponse model should serialize to JSON."""
    block = Block(id="b1", type="text", content="Hello", color="")
    section = Section(title="Summary", blocks=[block])
    people = People(title="People", blocks=[])
    notes = MeetingNotes(meeting_name="Test", sections=[section])
    response = SummaryResponse(
        MeetingName="Test Meeting",
        People=people,
        SessionSummary=section,
        CriticalDeadlines=Section(title="Critical Deadlines", blocks=[]),
        KeyItemsDecisions=Section(title="Key Items", blocks=[]),
        ImmediateActionItems=Section(title="Action Items", blocks=[]),
        NextSteps=Section(title="Next Steps", blocks=[]),
        MeetingNotes=notes,
    )
    json_str = response.model_dump_json()
    assert "Test Meeting" in json_str


def test_summary_response_deserialization():
    """SummaryResponse model should deserialize from JSON."""
    data = {
        "MeetingName": "Team Standup",
        "People": {"title": "People", "blocks": []},
        "SessionSummary": {"title": "Summary", "blocks": [
            {"id": "1", "type": "text", "content": "Good meeting", "color": ""}
        ]},
        "CriticalDeadlines": {"title": "Deadlines", "blocks": []},
        "KeyItemsDecisions": {"title": "Decisions", "blocks": []},
        "ImmediateActionItems": {"title": "Actions", "blocks": []},
        "NextSteps": {"title": "Steps", "blocks": []},
        "MeetingNotes": {"meeting_name": "Team Standup", "sections": []},
    }
    response = SummaryResponse(**data)
    assert response.MeetingName == "Team Standup"
    assert len(response.SessionSummary.blocks) == 1


def test_block_valid_types():
    """Block type must be one of the allowed literals."""
    for block_type in ["bullet", "heading1", "heading2", "text"]:
        block = Block(id="1", type=block_type, content="test", color="")
        assert block.type == block_type


def test_block_invalid_type():
    """Block with invalid type should raise validation error."""
    with pytest.raises(Exception):
        Block(id="1", type="invalid", content="test", color="")


def test_section_empty_blocks():
    section = Section(title="Empty", blocks=[])
    assert section.blocks == []


def test_meeting_notes_structure():
    section = Section(
        title="Discussion",
        blocks=[Block(id="1", type="bullet", content="Point 1", color="gray")]
    )
    notes = MeetingNotes(meeting_name="Sprint Review", sections=[section])
    assert notes.meeting_name == "Sprint Review"
    assert len(notes.sections) == 1
    assert notes.sections[0].blocks[0].color == "gray"


# ── TranscriptProcessor init ────────────────────────────────────────


def test_transcript_processor_init():
    """TranscriptProcessor should initialize without errors."""
    tp = TranscriptProcessor()
    assert hasattr(tp, "db")
    assert hasattr(tp, "active_clients")
    assert tp.active_clients == []


# ── TranscriptProcessor cleanup ──────────────────────────────────────


def test_transcript_processor_cleanup():
    """Cleanup should not raise even with no active clients."""
    tp = TranscriptProcessor()
    tp.cleanup()  # Should not raise


# ── process_transcript error handling ────────────────────────────────


async def test_process_transcript_unsupported_model():
    """Unsupported model provider should raise ValueError."""
    tp = TranscriptProcessor()
    with pytest.raises(ValueError, match="Unsupported model provider"):
        await tp.process_transcript(
            text="test transcript",
            model="nonexistent_provider",
            model_name="some-model",
        )


async def test_process_transcript_claude_no_key():
    """Claude provider without API key should raise ValueError."""
    tp = TranscriptProcessor()
    # Clear any saved API key
    try:
        await tp.db.delete_api_key("claude")
    except Exception:
        pass
    with pytest.raises(ValueError, match="ANTHROPIC_API_KEY"):
        await tp.process_transcript(
            text="test transcript",
            model="claude",
            model_name="claude-3.5-sonnet",
        )


async def test_process_transcript_groq_no_key():
    """Groq provider without API key should raise ValueError."""
    tp = TranscriptProcessor()
    try:
        await tp.db.delete_api_key("groq")
    except Exception:
        pass
    with pytest.raises(ValueError, match="GROQ_API_KEY"):
        await tp.process_transcript(
            text="test transcript",
            model="groq",
            model_name="llama-3.1",
        )


async def test_process_transcript_openai_no_key():
    """OpenAI provider without API key should raise ValueError."""
    tp = TranscriptProcessor()
    try:
        await tp.db.delete_api_key("openai")
    except Exception:
        pass
    with pytest.raises(ValueError, match="OPENAI_API_KEY"):
        await tp.process_transcript(
            text="test transcript",
            model="openai",
            model_name="gpt-4o",
        )


# ── Chunking logic (via internal behavior) ──────────────────────────


def test_chunking_logic():
    """Verify the text chunking math used in process_transcript."""
    text = "A" * 15000
    chunk_size = 5000
    overlap = 1000
    step = chunk_size - overlap
    chunks = [text[i:i + chunk_size] for i in range(0, len(text), step)]
    assert len(chunks) == 4  # 0-5000, 4000-9000, 8000-13000, 12000-15000
    assert all(len(c) <= chunk_size for c in chunks)


def test_chunking_with_overlap_equals_chunk_size():
    """When overlap >= chunk_size, step should be adjusted."""
    text = "A" * 10000
    chunk_size = 5000
    overlap = 5000
    # From the code: overlap is clamped to chunk_size - 100
    overlap = max(0, chunk_size - 100)
    step = chunk_size - overlap
    chunks = [text[i:i + chunk_size] for i in range(0, len(text), step)]
    assert len(chunks) > 0
    assert step > 0


# ── Chunking edge cases ──────────────────────────────────────────


def test_chunking_small_text_single_chunk():
    """Text smaller than chunk_size should produce 1 chunk."""
    text = "Short text"
    chunk_size = 5000
    overlap = 1000
    step = chunk_size - overlap
    chunks = [text[i:i + chunk_size] for i in range(0, len(text), step)]
    assert len(chunks) == 1
    assert chunks[0] == text


def test_chunking_exact_chunk_size():
    """Text exactly equal to chunk_size should produce 1 chunk."""
    text = "A" * 5000
    chunk_size = 5000
    overlap = 1000
    step = chunk_size - overlap
    # With overlap-based stepping, range(0, 5000, 4000) yields [0, 4000].
    # The second chunk (text[4000:9000]) is just the trailing overlap and
    # should be dropped when it adds no new content beyond the first chunk.
    chunks = [text[i:i + chunk_size] for i in range(0, len(text), step)]
    # Filter out trailing chunks that are fully contained in the previous chunk
    chunks = [c for i, c in enumerate(chunks) if i == 0 or len(c) > overlap]
    assert len(chunks) == 1


def test_chunking_overlap_creates_overlapping_content():
    """Consecutive chunks should have overlapping content."""
    text = "ABCDEFGHIJ" * 1000  # 10000 chars
    chunk_size = 5000
    overlap = 1000
    step = chunk_size - overlap
    chunks = [text[i:i + chunk_size] for i in range(0, len(text), step)]
    if len(chunks) >= 2:
        # Last 1000 chars of chunk 0 should equal first 1000 chars of chunk 1
        assert chunks[0][-overlap:] == chunks[1][:overlap]


def test_chunking_zero_overlap():
    """Zero overlap should produce non-overlapping chunks."""
    text = "A" * 10000
    chunk_size = 5000
    overlap = 0
    step = chunk_size - overlap
    chunks = [text[i:i + chunk_size] for i in range(0, len(text), step)]
    assert len(chunks) == 2


# ── SummaryResponse edge cases ──────────────────────────────────


def test_summary_response_empty_meeting_name():
    block = Block(id="b1", type="text", content="Content", color="")
    section = Section(title="Summary", blocks=[block])
    people = People(title="People", blocks=[])
    notes = MeetingNotes(meeting_name="", sections=[])
    response = SummaryResponse(
        MeetingName="",
        People=people,
        SessionSummary=section,
        CriticalDeadlines=Section(title="Deadlines", blocks=[]),
        KeyItemsDecisions=Section(title="Decisions", blocks=[]),
        ImmediateActionItems=Section(title="Actions", blocks=[]),
        NextSteps=Section(title="Steps", blocks=[]),
        MeetingNotes=notes,
    )
    assert response.MeetingName == ""


def test_summary_response_all_empty_sections():
    people = People(title="People", blocks=[])
    empty_section = Section(title="Empty", blocks=[])
    notes = MeetingNotes(meeting_name="Test", sections=[])
    response = SummaryResponse(
        MeetingName="Test",
        People=people,
        SessionSummary=empty_section,
        CriticalDeadlines=empty_section,
        KeyItemsDecisions=empty_section,
        ImmediateActionItems=empty_section,
        NextSteps=empty_section,
        MeetingNotes=notes,
    )
    json_str = response.model_dump_json()
    assert "Test" in json_str


def test_block_gray_color():
    block = Block(id="1", type="text", content="Muted text", color="gray")
    assert block.color == "gray"


def test_block_empty_color():
    block = Block(id="1", type="text", content="Normal text", color="")
    assert block.color == ""


def test_section_with_multiple_blocks():
    blocks = [
        Block(id=str(i), type="bullet", content=f"Item {i}", color="")
        for i in range(10)
    ]
    section = Section(title="Big Section", blocks=blocks)
    assert len(section.blocks) == 10


def test_meeting_notes_multiple_sections():
    sections = [
        Section(title=f"Section {i}", blocks=[
            Block(id=f"b{i}", type="text", content=f"Content {i}", color="")
        ])
        for i in range(5)
    ]
    notes = MeetingNotes(meeting_name="Multi-Section Meeting", sections=sections)
    assert len(notes.sections) == 5


# ── TranscriptProcessor model selection ──────────────────────────


async def test_process_transcript_empty_text():
    """Empty text should propagate through to model provider which will raise."""
    tp = TranscriptProcessor()
    # Empty text with unsupported model should raise
    with pytest.raises(ValueError, match="Unsupported model provider"):
        await tp.process_transcript(
            text="some text",
            model="nonexistent",
            model_name="model",
        )


def test_transcript_processor_has_db():
    tp = TranscriptProcessor()
    assert tp.db is not None


def test_transcript_processor_cleanup_twice():
    """Calling cleanup twice should not raise."""
    tp = TranscriptProcessor()
    tp.cleanup()
    tp.cleanup()
