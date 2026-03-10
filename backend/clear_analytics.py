import asyncio
import asyncpg
import os

async def clear_data():
    db_url = "postgresql://neondb_owner:npg_3JYK7ySezjrT@ep-morning-truth-ahrz730e-pooler.c-3.us-east-1.aws.neon.tech/neondb?sslmode=require"
    try:
        conn = await asyncpg.connect(db_url)
        await conn.execute("DELETE FROM analytics_events;")
        print("Successfully deleted all analytics_events data.")
        await conn.close()
    except Exception as e:
        print(f"Error: {e}")

asyncio.run(clear_data())
