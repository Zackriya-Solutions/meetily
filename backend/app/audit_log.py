"""
Structured audit logging to MongoDB.

Records auth events (login, failed_login, register, logout, password_reset, etc.)
with user context, IP address, and timestamps for security auditing.
"""

import logging
from datetime import datetime, timezone

from mongodb import get_audit_log_collection

logger = logging.getLogger(__name__)


async def log_event(
    event_type: str,
    user_id: str = None,
    email: str = None,
    ip: str = None,
    metadata: dict = None,
):
    """Write a structured audit log entry. Never raises — failures are logged."""
    try:
        await get_audit_log_collection().insert_one({
            "event_type": event_type,
            "user_id": user_id,
            "email": email,
            "ip": ip,
            "timestamp": datetime.now(timezone.utc),
            "metadata": metadata or {},
        })
    except Exception as e:
        logger.error(f"Failed to write audit log ({event_type}): {e}")
