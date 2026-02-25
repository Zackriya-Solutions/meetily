from fastapi import Depends, HTTPException, status
from fastapi.security import HTTPBearer, HTTPAuthorizationCredentials
import logging

logger = logging.getLogger(__name__)

# Robust import strategy for dual environment (Package vs Root)
try:
    # Try relative imports (works when running as package 'app')
    from ..core.security import verify_google_token, GOOGLE_CLIENT_ID
    from ..schemas.user import User
except (ImportError, ValueError):
    # Fallback for Docker root execution (where api/core/schemas are top-level)
    import sys

    # Ensure current dir is in path to find siblings
    sys.path.append(".")
    from core.security import verify_google_token, GOOGLE_CLIENT_ID
    from schemas.user import User

security = HTTPBearer()


async def get_current_user(
    credentials: HTTPAuthorizationCredentials = Depends(security),
) -> User:
    """
    Dependency to get the current authenticated user.
    Validates JWT token from Authorization header.
    """
    token = credentials.credentials

    if not GOOGLE_CLIENT_ID:
        logger.warning("DEBUG AUTH: GOOGLE_CLIENT_ID is None")

    try:
        payload = await verify_google_token(token)
        logger.info(f"DEBUG AUTH: Payload extracted for {payload.get('email')}")
    except HTTPException as e:
        raise e
    except Exception as e:
        logger.error(f"DEBUG AUTH: Unexpected verification error: {str(e)}")
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail=f"Could not validate credentials: {str(e)}",
        )

    email = payload.get("email")
    if not email:
        raise HTTPException(status_code=401, detail="Token missing email")

    # Domain restriction check
    # Temporary bypass for testing
    if not email.endswith("@appointy.com"):
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail=f"Access restricted to @appointy.com users (found {email})",
        )

    return User(email=email, name=payload.get("name"), picture=payload.get("picture"))
