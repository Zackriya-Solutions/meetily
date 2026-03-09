"""Tests for SchemaValidator — schema validation and automatic fixes."""

import pytest
import os
import sys
import sqlite3

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "app"))

from schema_validator import SchemaValidator


@pytest.fixture
def db_path(tmp_path):
    return str(tmp_path / "schema_test.db")


@pytest.fixture
def validator(db_path):
    """Create a SchemaValidator and initialize a minimal schema."""
    return SchemaValidator(db_path)


def _create_table(db_path, table_name, columns):
    """Create a table with given columns for testing."""
    with sqlite3.connect(db_path) as conn:
        cols = ", ".join(f"{name} {ctype}" for name, ctype in columns)
        conn.execute(f"CREATE TABLE IF NOT EXISTS {table_name} ({cols})")
        conn.commit()


# ── validate_schema on fresh DB ────────────────────────────────────


def test_validate_schema_no_tables(validator, db_path):
    """validate_schema should not raise when tables don't exist yet."""
    # Create the database file but no tables
    with sqlite3.connect(db_path) as conn:
        conn.execute("SELECT 1")
    validator.validate_schema()


def test_validate_schema_with_complete_tables(db_path):
    """validate_schema should pass when all expected columns exist."""
    # Initialize DB via DatabaseManager (which creates all tables)
    from db import DatabaseManager
    dm = DatabaseManager(db_path=db_path)
    # Re-validate should succeed
    validator = SchemaValidator(db_path)
    validator.validate_schema()


# ── _get_expected_schema ───────────────────────────────────────────


def test_expected_schema_has_all_tables(validator):
    schema = validator._get_expected_schema()
    expected_tables = [
        "meetings", "transcripts", "summary_processes",
        "transcript_chunks", "settings", "transcript_settings",
    ]
    for table in expected_tables:
        assert table in schema, f"Missing table '{table}' in expected schema"


def test_expected_schema_meetings_columns(validator):
    schema = validator._get_expected_schema()
    col_names = [col[0] for col in schema["meetings"]]
    assert "id" in col_names
    assert "title" in col_names
    assert "created_at" in col_names
    assert "updated_at" in col_names


def test_expected_schema_settings_columns(validator):
    schema = validator._get_expected_schema()
    col_names = [col[0] for col in schema["settings"]]
    assert "provider" in col_names
    assert "model" in col_names
    assert "whisperModel" in col_names
    assert "groqApiKey" in col_names
    assert "openaiApiKey" in col_names
    assert "anthropicApiKey" in col_names


# ── Missing column detection and addition ─────────────────────────


def test_detect_and_add_missing_column(validator, db_path):
    """Validator should add missing columns to existing tables."""
    # Create meetings table missing the 'updated_at' column
    with sqlite3.connect(db_path) as conn:
        conn.execute("""
            CREATE TABLE meetings (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                created_at TEXT NOT NULL
            )
        """)
        conn.commit()

    # validate_schema should add the missing column
    validator.validate_schema()

    # Verify column was added
    with sqlite3.connect(db_path) as conn:
        cursor = conn.execute("PRAGMA table_info(meetings)")
        col_names = [row[1] for row in cursor.fetchall()]
    assert "updated_at" in col_names


def test_detect_multiple_missing_columns(validator, db_path):
    """Validator should add multiple missing columns."""
    # Create settings table with only id and provider
    with sqlite3.connect(db_path) as conn:
        conn.execute("""
            CREATE TABLE settings (
                id TEXT PRIMARY KEY,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                whisperModel TEXT NOT NULL
            )
        """)
        conn.commit()

    validator.validate_schema()

    with sqlite3.connect(db_path) as conn:
        cursor = conn.execute("PRAGMA table_info(settings)")
        col_names = [row[1] for row in cursor.fetchall()]
    assert "groqApiKey" in col_names
    assert "openaiApiKey" in col_names
    assert "anthropicApiKey" in col_names
    assert "ollamaApiKey" in col_names


def test_validate_no_changes_needed(db_path):
    """If schema is complete, validate_schema should not modify anything."""
    from db import DatabaseManager
    dm = DatabaseManager(db_path=db_path)

    validator = SchemaValidator(db_path)
    # Should not raise or modify anything
    validator.validate_schema()


def test_validate_table_schema_nonexistent_table(validator, db_path):
    """_validate_table_schema should handle non-existent table gracefully."""
    with sqlite3.connect(db_path) as conn:
        conn.execute("SELECT 1")  # Create DB file
    # Should not raise
    with sqlite3.connect(db_path) as conn:
        cursor = conn.cursor()
        validator._validate_table_schema(cursor, "nonexistent_table", [
            ("col1", "TEXT", "")
        ])


def test_validate_adds_columns_to_transcripts(validator, db_path):
    """Validator should add missing columns to transcripts table."""
    with sqlite3.connect(db_path) as conn:
        conn.execute("""
            CREATE TABLE transcripts (
                id TEXT PRIMARY KEY,
                meeting_id TEXT NOT NULL,
                transcript TEXT NOT NULL,
                timestamp TEXT NOT NULL
            )
        """)
        conn.commit()

    validator.validate_schema()

    with sqlite3.connect(db_path) as conn:
        cursor = conn.execute("PRAGMA table_info(transcripts)")
        col_names = [row[1] for row in cursor.fetchall()]
    assert "summary" in col_names
    assert "action_items" in col_names
    assert "key_points" in col_names
