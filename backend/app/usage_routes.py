"""
Usage tracking API routes.

Provides batch event ingestion and usage summary queries.
All endpoints require authentication.
"""

import logging
from datetime import datetime, timezone
from typing import Optional

from fastapi import APIRouter, Depends, HTTPException, Query
from pymongo.errors import BulkWriteError

from mongodb import get_usage_events_collection, get_usage_summaries_collection
from auth_middleware import get_current_user
from usage_models import BatchUsageEventsRequest, UsageSummaryResponse
from usage_aggregator import aggregate_user_usage

logger = logging.getLogger(__name__)
router = APIRouter(prefix="/api/usage", tags=["usage"])


@router.post("/events")
async def ingest_events(
    req: BatchUsageEventsRequest,
    current_user: dict = Depends(get_current_user),
):
    """Batch ingest usage events from a device."""
    user_id = current_user["sub"]
    col = get_usage_events_collection()

    now = datetime.now(timezone.utc)
    docs = []
    for event in req.events:
        doc = {
            "user_id": user_id,
            "device_id": req.device_id,
            "event_type": event.event_type.value,
            "value": event.value,
            "metadata": event.metadata or {},
            "session_id": event.session_id,
            "timestamp": event.timestamp,
            "received_at": now,
        }
        if event.client_event_id:
            doc["client_event_id"] = event.client_event_id
        docs.append(doc)

    inserted = 0
    if docs:
        try:
            result = await col.insert_many(docs, ordered=False)
            inserted = len(result.inserted_ids)
        except BulkWriteError as bwe:
            inserted = bwe.details.get("nInserted", 0)
            dup_count = len(docs) - inserted
            if dup_count > 0:
                logger.info(
                    f"Skipped {dup_count} duplicate events for user {user_id}"
                )
        logger.info(
            f"Ingested {inserted} usage events for user {user_id} "
            f"device {req.device_id}"
        )

    # Trigger async aggregation for this user
    try:
        await aggregate_user_usage(user_id)
    except Exception as e:
        logger.warning(f"Aggregation failed (non-fatal): {e}")

    return {"ingested": inserted}


@router.get("/summary", response_model=UsageSummaryResponse)
async def get_usage_summary(
    period: str = Query("all_time", regex="^(daily|monthly|all_time)$"),
    period_start: Optional[str] = None,
    current_user: dict = Depends(get_current_user),
):
    """Get the authenticated user's usage summary."""
    user_id = current_user["sub"]
    return await _build_summary(user_id, device_id=None, period=period, period_start=period_start)


@router.get("/summary/{device_id}", response_model=UsageSummaryResponse)
async def get_device_usage_summary(
    device_id: str,
    period: str = Query("all_time", regex="^(daily|monthly|all_time)$"),
    period_start: Optional[str] = None,
    current_user: dict = Depends(get_current_user),
):
    """Get usage summary for a specific device."""
    user_id = current_user["sub"]
    return await _build_summary(user_id, device_id=device_id, period=period, period_start=period_start)


@router.get("/events")
async def query_events(
    limit: int = Query(50, ge=1, le=200),
    offset: int = Query(0, ge=0),
    device_id: Optional[str] = None,
    event_type: Optional[str] = None,
    current_user: dict = Depends(get_current_user),
):
    """Query raw usage events with pagination."""
    user_id = current_user["sub"]
    col = get_usage_events_collection()

    query = {"user_id": user_id}
    if device_id:
        query["device_id"] = device_id
    if event_type:
        query["event_type"] = event_type

    cursor = col.find(query).sort("received_at", -1).skip(offset).limit(limit)
    events = []
    async for doc in cursor:
        doc["_id"] = str(doc["_id"])
        # Convert datetimes to ISO strings
        for key in ("received_at",):
            if key in doc and hasattr(doc[key], "isoformat"):
                doc[key] = doc[key].isoformat()
        events.append(doc)

    total = await col.count_documents(query)
    return {"events": events, "total": total, "limit": limit, "offset": offset}


# ── Internal ─────────────────────────────────────────────────────────


async def _build_summary(
    user_id: str,
    device_id: Optional[str],
    period: str,
    period_start: Optional[str],
) -> UsageSummaryResponse:
    """Build a usage summary from the usage_summaries collection or raw events."""
    col = get_usage_summaries_collection()

    if period == "all_time":
        # Aggregate across all summaries for this user
        match = {"user_id": user_id}
        if device_id:
            match["device_id"] = device_id

        pipeline = [
            {"$match": match},
            {
                "$group": {
                    "_id": None,
                    "recording_minutes": {"$sum": "$recording_minutes"},
                    "transcription_minutes": {"$sum": "$transcription_minutes"},
                    "meetings_count": {"$sum": "$meetings_count"},
                    "summaries_count": {"$sum": "$summaries_count"},
                    "active_minutes": {"$sum": "$active_minutes"},
                    "sessions_count": {"$sum": "$sessions_count"},
                    "whisper_models": {"$addToSet": "$whisper_models_used"},
                    "llm_models": {"$push": "$llm_models_used"},
                    "devices": {"$addToSet": "$device_id"},
                }
            },
        ]

        results = []
        async for doc in col.aggregate(pipeline):
            results.append(doc)

        if not results:
            return UsageSummaryResponse(period=period)

        r = results[0]
        # Flatten nested whisper models
        whisper_flat = set()
        for models in r.get("whisper_models", []):
            if isinstance(models, list):
                whisper_flat.update(models)
            elif isinstance(models, str):
                whisper_flat.add(models)

        return UsageSummaryResponse(
            period=period,
            recording_minutes=r.get("recording_minutes", 0),
            transcription_minutes=r.get("transcription_minutes", 0),
            meetings_count=r.get("meetings_count", 0),
            summaries_count=r.get("summaries_count", 0),
            active_minutes=r.get("active_minutes", 0),
            sessions_count=r.get("sessions_count", 0),
            whisper_models_used=sorted(whisper_flat),
            device_count=len(r.get("devices", [])),
        )

    # Daily or monthly — look up specific period
    query = {"user_id": user_id, "period_type": period}
    if device_id:
        query["device_id"] = device_id
    if period_start:
        query["period_start"] = period_start

    doc = await col.find_one(query, sort=[("period_start", -1)])
    if not doc:
        return UsageSummaryResponse(period=period, period_start=period_start)

    return UsageSummaryResponse(
        period=period,
        period_start=doc.get("period_start"),
        recording_minutes=doc.get("recording_minutes", 0),
        transcription_minutes=doc.get("transcription_minutes", 0),
        meetings_count=doc.get("meetings_count", 0),
        summaries_count=doc.get("summaries_count", 0),
        active_minutes=doc.get("active_minutes", 0),
        sessions_count=doc.get("sessions_count", 0),
        whisper_models_used=doc.get("whisper_models_used", []),
        device_count=1 if device_id else 0,
    )
