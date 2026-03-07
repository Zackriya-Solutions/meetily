# Clearminutes API Documentation

## Prerequisites

### System Requirements
- Python 3.8 or higher
- pip (Python package installer)
- SQLite 3
- Sufficient disk space for database and transcript storage

### Required Environment Variables
Create a `.env` file in the backend directory with the following variables:
```env
# API Keys
ANTHROPIC_API_KEY=your_anthropic_api_key    # Required for Claude model
GROQ_API_KEY=your_groq_api_key              # Optional, for Groq model

# Database Configuration
DB_PATH=./meetings.db                        # SQLite database path

# Server Configuration
HOST=0.0.0.0                                # Server host
PORT=5167                                   # Server port

# Processing Configuration
CHUNK_SIZE=5000                             # Default chunk size for processing
CHUNK_OVERLAP=1000                          # Default overlap between chunks
```

### Installation

1. Create and activate a virtual environment:
```bash
python -m venv venv
source venv/bin/activate  # On Windows: venv\Scripts\activate
```

2. Install required packages:
```bash
pip install -r requirements.txt
```

Required packages:
- pydantic
- pydantic-ai==0.0.19
- pandas
- devtools
- chromadb
- python-dotenv
- fastapi
- uvicorn
- python-multipart
- aiosqlite

3. Initialize the database:
```bash
python -c "from app.db import init_db; import asyncio; asyncio.run(init_db())"
```

### Running the Server

Start the server using uvicorn:
```bash
uvicorn app.main:app --host 0.0.0.0 --port 5167 --reload
```

The API will be available at `http://localhost:5167`

## Project Structure
```
backend/
├── app/
│   ├── __init__.py
│   ├── main.py              # Main FastAPI application
│   ├── db.py               # Database operations
│   └── transcript_processor.py.py # Transcript processing logic
├── requirements.txt         # Python dependencies
└── meeting_minutes.db             # SQLite database
```

## Overview
This API provides endpoints for processing meeting transcripts and generating structured summaries. It uses AI models to analyze transcripts and extract key information such as action items, decisions, and deadlines.

## Base URL
```
http://localhost:5167
```

## Authentication
Currently, no authentication is required for API endpoints.

## Endpoints

### 1. Process Transcript
Process a transcript text directly.

**Endpoint:** `/process-transcript`  
**Method:** POST  
**Content-Type:** `application/json`

#### Request Body
```json
{
    "text": "string",           // Required: The transcript text
    "model": "string",          // Required: AI model to use (e.g., "ollama")
    "model_name": "string",     // Required: Model version (e.g., "qwen2.5:14b")
    "chunk_size": 40000,         // Optional: Size of text chunks (default: 80000)
    "overlap": 1000             // Optional: Overlap between chunks (default: 1000)
}
```

#### Response
```json
{
    "process_id": "string",
    "message": "Processing started"
}
```

### 2. Upload Transcript
Upload and process a transcript file. This endpoint provides the same functionality as `/process-transcript` but accepts a file upload instead of raw text.

**Endpoint:** `/upload-transcript`  
**Method:** POST  
**Content-Type:** `multipart/form-data`

#### Request Parameters
| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| file | File | Yes | The transcript file to upload |
| model | String | No | AI model to use (default: "claude") |
| model_name | String | No | Specific model version (default: "claude-3-5-sonnet-latest") |
| chunk_size | Integer | No | Size of text chunks (default: 5000) |
| overlap | Integer | No | Overlap between chunks (default: 1000) |

#### Response
```json
{
    "process_id": "string",
    "message": "Processing started"
}
```

### 3. Get Summary
Retrieve the generated summary for a specific process.

**Endpoint:** `/get-summary/{process_id}`  
**Method:** GET

#### Path Parameters
| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| process_id | String | Yes | ID of the process to retrieve |

#### Response Codes
| Code | Description |
|------|-------------|
| 200 | Success - Summary completed |
| 202 | Accepted - Processing in progress |
| 400 | Bad Request - Failed or unknown status |
| 404 | Not Found - Process ID not found |
| 500 | Internal Server Error - Server-side error |

#### Response Body
```json
{
    "status": "string",       // "completed", "processing", "error"
    "meetingName": "string",  // Name of the meeting (null if not available)
    "process_id": "string",   // Process ID
    "data": {                 // Summary data (null if not completed)
        "MeetingName": "string",
        "SectionSummary": {
            "title": "string",
            "blocks": [
                {
                    "id": "string",
                    "type": "string",
                    "content": "string",
                    "color": "string"
                }
            ]
        },
        "CriticalDeadlines": {
            "title": "string",
            "blocks": []
        },
        "KeyItemsDecisions": {
            "title": "string",
            "blocks": []
        },
        "ImmediateActionItems": {
            "title": "string",
            "blocks": []
        },
        "NextSteps": {
            "title": "string",
            "blocks": []
        },
        "OtherImportantPoints": {
            "title": "string",
            "blocks": []
        },
        "ClosingRemarks": {
            "title": "string",
            "blocks": []
        }
    },
    "start": "string",      // Start time in ISO format (null if not started)
    "end": "string",        // End time in ISO format (null if not completed)
    "error": "string"       // Error message if status is "error"
}

## Data Models

### Block
Represents a single block of content in a section.

```json
{
    "id": "string",      // Unique identifier
    "type": "string",    // Type of block (text, action, decision, etc.)
    "content": "string", // Content text
    "color": "string"    // Color for UI display
}
```

### Section
Represents a section in the meeting summary.

```json
{
    "title": "string",   // Section title
    "blocks": [          // Array of Block objects
        {
            "id": "string",
            "type": "string",
            "content": "string",
            "color": "string"
        }
    ]
}
```

## Status Codes

| Code | Description |
|------|-------------|
| 200 | Success - Request completed successfully |
| 202 | Accepted - Processing in progress |
| 400 | Bad Request - Invalid request or parameters |
| 404 | Not Found - Process ID not found |
| 500 | Internal Server Error - Server-side error |

## Error Handling
All error responses follow this format:
```json
{
    "status": "error",
    "meetingName": null,
    "process_id": "string",
    "data": null,
    "start": null,
    "end": null,
    "error": "Error message describing what went wrong"
}
```

## Example Usage

### 1. Upload and Process a Transcript
```bash
curl -X POST -F "file=@transcript.txt" http://localhost:5167/upload-transcript
```

### 2. Check Processing Status
```bash
curl http://localhost:5167/get-summary/1a2e5c9c-a35f-452f-9f92-be66620fcb3f
```

## Notes
1. Large transcripts are automatically chunked for processing
2. Processing times may vary based on transcript length
3. All timestamps are in ISO format
4. Colors in blocks can be used for UI styling
5. The API supports concurrent processing of multiple transcripts
