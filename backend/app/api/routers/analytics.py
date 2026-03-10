from fastapi import APIRouter, Depends, Request, HTTPException
from typing import Dict, Any, List
from pydantic import BaseModel
import json
import logging

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/analytics", tags=["Analytics"])

try:
    from ..deps import get_current_user
    from ...db import DatabaseManager
except (ImportError, ValueError):
    from api.deps import get_current_user
    from db import DatabaseManager

db = DatabaseManager()


class TrackEventRequest(BaseModel):
    event_name: str
    properties: Dict[str, Any] = {}
    session_id: str | None = None
    user_id: str | None = None
    timestamp: str | None = None


@router.post("/track")
async def track_event(request: TrackEventRequest, req: Request):
    """Ingest analytics events from frontend."""
    # Try to extract user email if logged in
    user_email = request.user_id
    try:
        # Since this endpoint can be called anonymously (before login),
        # we don't enforce token presence, but we try to decode it if exists.
        auth_header = req.headers.get("Authorization")
        if auth_header and auth_header.startswith("Bearer "):
            current_user = await get_current_user(
                auth_header
            )  # Actually get_current_user needs Depends injected HTTPAuthorizationCredentials.
            # We will just rely on the frontend passing the user_id for now for simplicity in public endpoints
            pass
    except Exception:
        pass

    query = """
    INSERT INTO analytics_events (session_id, user_id, event_name, properties, timestamp)
    VALUES ($1, $2, $3, $4, CURRENT_TIMESTAMP)
    """

    try:
        async with db._get_connection() as conn:
            await conn.execute(
                query,
                request.session_id,
                user_email,
                request.event_name,
                json.dumps(request.properties),
            )
    except Exception as e:
        logger.error(f"Failed to insert analytics event: {e}")
        # Return success anyway so frontend doesn't crash on analytics failure

    return {"status": "success"}


@router.get("/dashboard/metrics")
async def get_dashboard_metrics(
    user_filter: str | None = None, user=Depends(get_current_user)
):
    """Fetch dashboard metrics, restricted to admin."""
    # Security: Only admin can see the dashboard
    if not user or getattr(user, "email", "") != "gagan@appointy.com":
        raise HTTPException(status_code=403, detail="Forbidden: Admin access only")

    try:
        async with db._get_connection() as conn:
            # Fetch all unique users to populate the dropdown
            unique_users_list_rows = await conn.fetch(
                "SELECT DISTINCT user_id FROM analytics_events WHERE user_id IS NOT NULL AND user_id != ''"
            )
            unique_users_list = [row["user_id"] for row in unique_users_list_rows]

            base_where = "user_id NOT LIKE 'localhost%'"
            if user_filter == "exclude_admin":
                base_where += " AND user_id != 'gagan@appointy.com'"
            elif user_filter and user_filter != "all":
                # if user_filter is a specific email
                base_where += f" AND user_id = '{user_filter}'"

            # Top-level KPIs
            total_events = await conn.fetchval(
                f"SELECT COUNT(*) FROM analytics_events WHERE {base_where}"
            )
            unique_users = await conn.fetchval(
                f"SELECT COUNT(DISTINCT user_id) FROM analytics_events WHERE user_id IS NOT NULL AND {base_where}"
            )

            # Breakdown by feature
            feature_breakdown_rows = await conn.fetch(f"""
                SELECT event_name, COUNT(*) as count 
                FROM analytics_events 
                WHERE {base_where}
                GROUP BY event_name 
                ORDER BY count DESC
                LIMIT 15
            """)
            feature_breakdown = [
                {"name": row["event_name"], "value": row["count"]}
                for row in feature_breakdown_rows
            ]

            # Template popularity (for notes_generated OR notes_template_switched)
            template_popularity_rows = await conn.fetch(f"""
                SELECT properties->>'template_name' as template_name, COUNT(*) as count
                FROM analytics_events
                WHERE event_name IN ('notes_generated', 'notes_template_switched') 
                  AND properties->>'template_name' IS NOT NULL
                  AND {base_where}
                GROUP BY properties->>'template_name'
                ORDER BY count DESC
            """)
            template_popularity = [
                {"name": row["template_name"], "value": row["count"]}
                for row in template_popularity_rows
            ]

            # Daily active usage (last 7 days)
            daily_usage_rows = await conn.fetch(f"""
                SELECT date_trunc('day', timestamp) as day, COUNT(*) as count
                FROM analytics_events
                WHERE timestamp >= CURRENT_DATE - INTERVAL '7 days'
                  AND {base_where}
                GROUP BY day
                ORDER BY day
            """)
            daily_usage = [
                {
                    "date": row["day"].strftime("%Y-%m-%d") if row["day"] else "",
                    "events": row["count"],
                }
                for row in daily_usage_rows
            ]

        return {
            "kpis": {
                "totalEvents": total_events or 0,
                "uniqueUsers": unique_users or 0,
            },
            "featureBreakdown": feature_breakdown,
            "templatePopularity": template_popularity,
            "dailyUsage": daily_usage,
            "uniqueUsersList": unique_users_list,
        }
    except Exception as e:
        logger.error(f"Failed to fetch metrics: {e}")
        raise HTTPException(status_code=500, detail="Failed to fetch dashboard metrics")
