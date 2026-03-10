import logging
import os
import asyncio
from typing import List, Dict, Optional
from dotenv import load_dotenv

# LLM Providers
from openai import AsyncOpenAI
from groq import AsyncGroq
from anthropic import AsyncAnthropic
# from ollama import AsyncClient

try:
    from ..db import DatabaseManager
    from ..schemas.summary import SummaryResponse
    from .gemini_client import (
        generate_content_text_async,
        generate_content_text_sync,
        stream_content_text_async,
    )
except (ImportError, ValueError):
    from db import DatabaseManager
    from schemas.summary import SummaryResponse
    from services.gemini_client import (
        generate_content_text_async,
        generate_content_text_sync,
        stream_content_text_async,
    )

# Set up logging
logger = logging.getLogger(__name__)

load_dotenv()


class ChatService:
    """Handles chat interactions with meeting context and web search."""

    def __init__(self, db: DatabaseManager):
        self.db = db
        self.active_clients = []

    async def _reformulate_query(
        self,
        question: str,
        history: List[Dict[str, str]],
        user_email: Optional[str] = None,
    ) -> str:
        """
        Reformulate the user's question into a standalone query using conversation history.
        Resolves pronouns like "it", "that", "these things" to their actual context.
        """
        if not history or len(history) == 0:
            return question

        try:
            # We use Gemini for fast reformulation
            # Prioritize Environment Variables over Database
            api_key = os.getenv("GEMINI_API_KEY") or os.getenv("GOOGLE_API_KEY")
            if not api_key:
                api_key = await self.db.get_api_key("gemini", user_email=user_email)
            if not api_key:
                return question

            # Format recent history (last 3 turns is usually enough for context)
            recent_history = history[-6:] if len(history) > 6 else history
            history_text = ""
            for msg in recent_history:
                role = msg.get("role", "user")
                content = msg.get("content", "")[:300]  # Truncate for speed
                history_text += f"{role.upper()}: {content}\n"

            prompt = f"""Given the conversation history, rewrite the last user question to be a concise, keyword-focused search query suitable for a search engine (like Google).
Resolve pronouns (it, that, they) and vague references.
If the user refers to "the previous question", replace it with the ACTUAL topic of the previous question.
Remove conversational filler like "can you tell me", "please search for", "how to do", etc., unless "how to" is part of the technical query.

Examples:
History: User: "How to fix deployment?" AI: "Use load balancing."
User: "Search web for that."
Result: "load balancing deployment fix"

History: User: "What is the price of BTC?" AI: "$95k."
User: "I meant search on web for the previous question."
Result: "current bitcoin price"

History: User: "can you tell me how to do proper load testing"
Result: "proper load testing guide best practices"

Do NOT answer the question. Just rewrite it as a search query.

History:
{history_text}

Last User Question: "{question}"

Search Query:"""

            reformulated = (
                await generate_content_text_async(
                    api_key=api_key,
                    model="gemini-3-pro-preview",
                    contents=prompt,
                )
            ).strip()

            # Sanity check: if result is too long or empty, fallback
            if not reformulated or len(reformulated) > len(question) * 4:
                return question

            logger.info(f"Query Reformulation: '{question}' -> '{reformulated}'")
            return reformulated

        except Exception as e:
            logger.warning(f"Query reformulation failed: {e}")
            return question

    async def _needs_web_search(
        self, question: str, context_snippet: str, user_email: Optional[str] = None
    ) -> bool:
        """
        Use gemini-pro model to quickly determine if the question needs a web search.
        Returns True if the question requires external/real-time information.
        """
        # First check if context is empty - if so, more likely to need web search
        has_context = bool(context_snippet and len(context_snippet.strip()) > 50)

        try:
            # Prioritize Environment Variables over Database
            api_key = os.getenv("GEMINI_API_KEY") or os.getenv("GOOGLE_API_KEY")
            if not api_key:
                api_key = await self.db.get_api_key("gemini", user_email=user_email)
            if not api_key:
                return False  # Default to no search if we can't classify

            classifier_prompt = f"""You are a classifier. Determine if this question requires REAL-TIME WEB SEARCH or can be answered from meeting context.

Question: "{question}"

Meeting context available: {"Yes, about: " + context_snippet[:200] if has_context else "No meeting context"}

Answer ONLY "SEARCH" or "MEETING":
- "SEARCH" if question asks about: weather, current events, latest news, stock prices, real-time data, or anything NOT in meeting notes
- "MEETING" if question can be answered from meeting discussion, action items, decisions, or participants"""

            answer = (
                (
                    generate_content_text_sync(
                        api_key=api_key,
                        model="gemini-3-pro-preview",
                        contents=classifier_prompt,
                    )
                )
                .strip()
                .upper()
            )
            needs_search = "SEARCH" in answer
            logger.info(
                f"Web search classifier: '{answer}' -> needs_web_search={needs_search}"
            )
            return needs_search

        except Exception as e:
            logger.warning(f"Web search classifier failed: {e}")
            return False

    async def _needs_linked_context(
        self, question: str, current_context_snippet: str
    ) -> bool:
        """
        Determine if the question needs linked meeting context using keyword detection.
        """
        question_lower = question.lower()

        # Keywords that trigger linked meeting search
        cross_meeting_keywords = [
            "search in linked meetings",
            "linked meetings",
            "linked meeting",
            "linked one",
            "search linked",
            "previous meeting",
            "last meeting",
            "other meeting",
            "earlier meeting",
            "compare",
            "comparison",
            "different from",
            "changed since",
            "history",
            "past",
            "previously discussed",
            "follow up",
            "follow-up",
            "what did we say",
            "what was said",
            "mentioned before",
            "discussed earlier",
        ]

        for keyword in cross_meeting_keywords:
            if keyword in question_lower:
                logger.info(
                    f"Linked context: keyword '{keyword}' detected, will fetch linked meeting context"
                )
                return True

        logger.info(
            "Linked context: no trigger keyword detected, skipping linked meeting search"
        )
        return False

    async def search_web(self, query: str, user_email: Optional[str] = None) -> str:
        """
        Real web search using SerpAPI (Google) + crawling + Gemini summarization.
        """
        logger.info(f"Real web search for: {query}")
        try:
            import httpx
            import trafilatura

            # Step 1: Search Google via SerpAPI
            SERPAPI_KEY = os.getenv("SERPAPI_KEY")

            async def serpapi_search():
                try:
                    from serpapi import GoogleSearch

                    params = {
                        "q": query,
                        "api_key": SERPAPI_KEY,
                        "num": 5,
                        "hl": "en",  # English language
                        "gl": "us",  # US region
                    }

                    # Run in thread pool since SerpAPI is sync
                    def do_search():
                        search = GoogleSearch(params)
                        results = search.get_dict()
                        return results.get("organic_results", [])

                    loop = asyncio.get_event_loop()
                    return await loop.run_in_executor(None, do_search)
                except Exception as e:
                    logger.error(f"SerpAPI search failed: {e}")
                    return []

            search_results = await serpapi_search()

            if not search_results:
                return f"No search results found for '{query}'."

            logger.info(f"SerpAPI found {len(search_results)} results")

            # Step 2: Crawl pages and extract content
            async def fetch_and_extract(url: str) -> dict:
                try:
                    async with httpx.AsyncClient(
                        timeout=10.0, follow_redirects=True
                    ) as client:
                        response = await client.get(
                            url,
                            headers={
                                "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36"
                            },
                        )
                        if response.status_code == 200:
                            html = response.text
                            # Extract main content using trafilatura
                            text = trafilatura.extract(
                                html, include_comments=False, include_tables=True
                            )
                            if text and len(text) > 100:
                                return {
                                    "url": url,
                                    "content": text[:2000],
                                    "success": True,
                                }
                except Exception as e:
                    logger.warning(f"Failed to crawl {url}: {e}")
                return {"url": url, "content": "", "success": False}

            # Crawl pages in parallel - SerpAPI uses 'link' instead of 'href'
            crawl_tasks = [
                fetch_and_extract(r.get("link", "")) for r in search_results[:4]
            ]
            crawled = await asyncio.gather(*crawl_tasks)

            # Collect successful extractions
            sources = []
            for i, result in enumerate(crawled):
                if result["success"] and result["content"]:
                    title = search_results[i].get("title", "Unknown")
                    sources.append(
                        {
                            "title": title,
                            "url": result["url"],
                            "content": result["content"],
                        }
                    )

            if not sources:
                # Fallback: use SerpAPI snippets
                sources = [
                    {
                        "title": r.get("title", "Unknown"),
                        "url": r.get("link", ""),
                        "content": r.get("snippet", ""),
                    }
                    for r in search_results[:3]
                ]

            logger.info(f"Extracted content from {len(sources)} sources")

            # Step 3: Use Gemini to synthesize
            # Prioritize Environment Variables over Database
            api_key = os.getenv("GEMINI_API_KEY") or os.getenv("GOOGLE_API_KEY")
            if not api_key:
                api_key = await self.db.get_api_key("gemini", user_email=user_email)

            if not api_key:
                return "❌ Gemini API key not configured."

            # Format sources for context
            sources_text = ""
            for i, src in enumerate(sources, 1):
                sources_text += f"\n[Source {i}: {src['title']}]\nURL: {src['url']}\nContent:\n{src['content']}\n---\n"

            prompt = f"""You are a research assistant for a meeting copilot. Provide a concise, factual answer to the query based on the web sources provided.

Query: {query}

Web Sources:
{sources_text}

Instructions:
1. Answer the query directly using ONLY information from the provided sources
2. Do NOT use inline citations (e.g. [Source 1]) in the text.
3. If sources conflict, acknowledge both perspectives and note the discrepancy
4. Prioritize recent information and authoritative sources
5. If sources don't adequately answer the query, clearly state what's missing
6. Paraphrase information in your own words - do NOT copy text verbatim from sources
7. Keep the response concise and meeting-appropriate (aim for 150-300 words unless the query requires more detail)
8. Use formatting sparingly - only use bullet points if listing distinct items; otherwise use clear prose
9. If asked about current statistics or data, include the date/timeframe from the source

Format: Provide a direct answer followed by supporting details without inline citations."""

            response_text = await generate_content_text_async(
                api_key=api_key,
                model="gemini-3-pro-preview",
                contents=prompt,
                config={
                    "temperature": 0.3,
                    "max_output_tokens": 2048,
                },
            )

            if response_text:
                return f"**🔍 Web Research Results:**\n\n{response_text}"
            else:
                return "Failed to generate summary from sources."

        except Exception as e:
            logger.error(f"Real web search failed: {e}", exc_info=True)
            return f"Web search failed: {str(e)}"

    async def chat_about_meeting(
        self,
        context: str,
        question: str,
        model: str,
        model_name: str,
        allowed_meeting_ids: Optional[List[str]] = None,
        history: Optional[List[Dict[str, str]]] = None,
        user_email: Optional[str] = None,
    ):
        """
        Ask a question about the meeting context with cross-meeting search capabilities.
        Returns a streaming response generator.
        """
        logger.info(f"Chat request: '{question}' using model {model}:{model_name}")

        # === 0. CONTEXTUAL REFORMULATION ===
        reformulated_question = question
        if history and len(history) > 0:
            reformulated_question = await self._reformulate_query(
                question, history, user_email
            )

        logic_question = reformulated_question

        # Initialize context containers
        web_context = ""
        cross_meeting_context = ""
        search_status_msg = ""

        # === 1. WEB SEARCH STRATEGY ===
        question_lower = logic_question.lower().strip()

        # A. Explicit Triggers
        web_search_triggers = [
            "search on web",
            "find on web",
            "search for",
            "google for",
        ]
        needs_search = False
        search_query = logic_question

        # SPECIAL HANDLING: Check if user is asking to search for a "previous" thing explicitly
        meta_references = [
            "previous question",
            "last question",
            "that question",
            "earlier question",
        ]
        is_meta_query = any(ref in question_lower for ref in meta_references)

        for trigger in web_search_triggers:
            if trigger in question_lower:
                logger.info(f"Auto-detected explicit web search trigger: '{trigger}'")
                needs_search = True

                if is_meta_query:
                    clean_reformulated = logic_question.lower()
                    for t in web_search_triggers:
                        clean_reformulated = clean_reformulated.replace(t, "")
                    search_query = clean_reformulated.strip()
                    if len(search_query) < 3:
                        search_query = logic_question
                else:
                    search_query_lower = logic_question.lower()
                    temp_query = search_query_lower
                    for t in web_search_triggers:
                        if t in temp_query:
                            temp_query = temp_query.replace(t, "")

                    search_query = temp_query.strip()
                    if len(search_query) < 3:
                        search_query = logic_question
                break

        # B. Implicit Triggers (Router decides)
        if not needs_search:
            needs_search = await self._needs_web_search(
                logic_question, context[:1000] if context else "", user_email=user_email
            )
            if needs_search:
                logger.info("Router decided this question needs Web Search.")
                search_query = logic_question

        # Execute Search if needed
        if needs_search:
            search_status_msg = f"🔍 Searching web for: *{search_query}*...\n\n"
            try:
                web_result = await self.search_web(search_query, user_email=user_email)
                if "Failed" not in web_result and "No search results" not in web_result:
                    web_context = f"\n\n=== EXTERNAL WEB CONTEXT ===\n{web_result}\n"
                else:
                    logger.warning(
                        f"Web search yielded no useful results: {web_result}"
                    )
            except Exception as e:
                logger.error(f"Web search failed during chat: {e}")

        # === 2. GLOBAL MEETING SEARCH STRATEGY ===
        global_search_triggers = [
            "search all meetings",
            "search in all meetings",
            "search globally",
            "global search",
            "find in all meetings",
            "search across meetings",
            "search in meetings",
            "search meetings",
            "search all",
        ]

        allow_global_search = False
        for trigger in global_search_triggers:
            if trigger in question_lower:
                logger.info(f"Auto-detected global meeting search trigger: '{trigger}'")
                allow_global_search = True
                break

        # === 3. FETCH ADDITIONAL CONTEXT ===

        # A. Global Search
        if allow_global_search:
            try:
                # NOTE: Vector DB usage might be conditional
                # For now assuming vector_store might be unavailable or we need to handle it gracefully
                # Since I haven't moved vector_store yet, I'll assume it exists in app.services or similar
                # But vector_store.py is in root backend/app.
                # I will import it conditionally or relative.

                from app.vector_store import (
                    search_context,
                    get_collection_stats,
                )  # Try this relative import later

                stats = get_collection_stats()
                if stats.get("status") == "available":
                    results = await search_context(
                        query=logic_question,
                        n_results=20,
                        allowed_meeting_ids=None,
                    )
                    logger.info(
                        f"Global meeting search: {len(results) if results else 0} chunks found"
                    )
                    if results:
                        cross_meeting_context += (
                            "\n\nRelevant Context from All Meetings (Global Search):\n"
                        )
                        for r in results:
                            source = f"{r.get('meeting_title', 'Unknown')} ({r.get('meeting_date', 'Unknown')})"
                            text = r.get("text", "").strip()
                            cross_meeting_context += f"- [{source}]: {text}\n"
            except ImportError:
                logger.warning("Vector store module not found, skipping global search")
            except Exception as e:
                logger.warning(f"Failed to perform global meeting search: {e}")

        # B. Linked Meetings (Context Window Management)
        elif allowed_meeting_ids and len(allowed_meeting_ids) > 0:
            needs_linked = (
                True  # If user manually linked meetings, assume they want to use them
            )

            if needs_linked:
                logger.info(
                    f"Fetching context for {len(allowed_meeting_ids)} linked meetings"
                )
                try:
                    linked_transcripts = []
                    total_chars = 0
                    MAX_LINKED_CHARS = 40000  # Cap around ~10k tokens for linked context to prevent context window blowup

                    for meeting_id in allowed_meeting_ids:
                        meeting_data = await self.db.get_meeting(meeting_id)
                        if meeting_data:
                            meeting_title = meeting_data.get("title", "Unknown Meeting")
                            meeting_date = meeting_data.get(
                                "created_at", "Unknown Date"
                            )
                            transcripts = meeting_data.get("transcripts", [])
                            full_transcript = "\n".join(
                                [t.get("text", "") for t in transcripts]
                            ).strip()

                            if full_transcript:
                                linked_transcripts.append(
                                    (meeting_title, meeting_date, full_transcript)
                                )
                                total_chars += len(full_transcript)

                    if total_chars > MAX_LINKED_CHARS:
                        # Fallback to vector search for linked meetings
                        logger.info(
                            f"Linked context too large ({total_chars} chars). Falling back to vector search."
                        )
                        vector_search_success = False
                        try:
                            from app.vector_store import (
                                search_context,
                                get_collection_stats,
                            )

                            stats = get_collection_stats()
                            if stats.get("status") == "available":
                                results = await search_context(
                                    query=logic_question,
                                    n_results=15,
                                    allowed_meeting_ids=allowed_meeting_ids,
                                )
                                if results:
                                    cross_meeting_context += "\n\n=== CROSS-MEETING CONTEXT (RELEVANT EXCERPTS) ===\n"
                                    for r in results:
                                        source = f"{r.get('meeting_title', 'Unknown')} ({r.get('meeting_date', 'Unknown')})"
                                        text = r.get("text", "").strip()
                                        cross_meeting_context += (
                                            f"- [{source}]: {text}\n"
                                        )
                                    vector_search_success = True
                        except ImportError:
                            logger.warning(
                                "Vector store missing during linked context fallback."
                            )
                        except Exception as ve:
                            logger.warning(
                                f"Vector search failed during linked context fallback: {ve}"
                            )

                        if not vector_search_success and linked_transcripts:
                            logger.info(
                                "Truncating linked transcripts proportionally as a last resort."
                            )
                            cross_meeting_context += "\n\n=== CROSS-MEETING CONTEXT (PARTIAL TRANSCRIPTS) ===\n"
                            chars_per_meeting = MAX_LINKED_CHARS // len(
                                linked_transcripts
                            )
                            for title, date, transcript in linked_transcripts:
                                cross_meeting_context += f"\n[{title}] ({date})\n"
                                cross_meeting_context += (
                                    transcript[:chars_per_meeting]
                                    + "\n...[TRUNCATED]...\n"
                                )
                    elif linked_transcripts:
                        cross_meeting_context += (
                            "\n\n=== CROSS-MEETING CONTEXT (FULL TRANSCRIPTS) ===\n"
                        )
                        for title, date, transcript in linked_transcripts:
                            cross_meeting_context += f"\n[{title}] ({date})\n"
                            cross_meeting_context += transcript + "\n"

                except Exception as e:
                    logger.warning(f"Failed to fetch linked meeting context: {e}")

        # === 4. CONSTRUCT PROMPT ===
        history_text = ""
        if history and len(history) > 0:
            history_text = "\nConversation History:\n"
            selected_history = (
                history if len(history) <= 10 else history[:2] + history[-8:]
            )
            for msg in selected_history:
                role = msg.get("role", "user")
                content = msg.get("content", "")[:1000]
                history_text += f"{role.upper()}: {content}\n"

        system_prompt = f"""You are an expert executive meeting assistant. Analyze the provided context meticulously to answer the user's questions.

CORE DIRECTIVES:
1. SYNTHESIS: Answer by synthesizing information across the 'CURRENT MEETING CONTEXT' and any provided 'CROSS-MEETING CONTEXT' (which may contain full transcripts or relevant excerpts from past meetings).
2. CONFLICT RESOLUTION: If current meeting decisions contradict past meetings, prioritize the CURRENT meeting as the source of truth, but explicitly note the change or progression.
3. FACT-CHECKING: If 'EXTERNAL WEB CONTEXT' is present, use it to verify external claims, elaborate on subjects, or answer off-topic questions not covered by the meeting.
4. CITATIONS: 
   - When citing the current meeting, use [Current Meeting].
   - When citing a past/linked meeting, explicitly use its title: [Meeting Title] (Date).
   - When citing a web source, use [Source N] or [Domain].
5. STRICT SCOPE: Do NOT hallucinate. If the answer cannot be confidently derived from the provided contexts, clearly state "I do not have enough information from the meeting context to answer that."

CONTEXTS:

=== CURRENT MEETING CONTEXT ===
{context}
================================
{cross_meeting_context}
{web_context}
{history_text}

USER QUESTION: {question}
"""

        # === 5. STREAM RESPONSE ===
        async def response_wrapper(generator):
            if search_status_msg:
                yield search_status_msg
            async for chunk in generator:
                yield chunk

        try:
            # --- OLLAMA SUPPORT ---
            # --- OLLAMA SUPPORT ---
            # if model == "ollama":
            #     ollama_host = os.getenv("OLLAMA_HOST", "http://localhost:11434")
            #     client = AsyncClient(host=ollama_host)
            #     message = {"role": "user", "content": question}
            #     system_message = {"role": "system", "content": system_prompt}

            #     async def stream_ollama():
            #         async for part in await client.chat(
            #             model=model_name,
            #             messages=[system_message, message],
            #             stream=True,
            #         ):
            #             content = part["message"]["content"]
            #             yield content

            #     return response_wrapper(stream_ollama())

            # --- GROQ SUPPORT ---
            if model == "groq":
                api_key = await self.db.get_api_key("groq", user_email=user_email)
                if not api_key:
                    api_key = os.getenv("GROQ_API_KEY")
                if not api_key:
                    raise ValueError("Groq API key not found.")

                client = AsyncGroq(api_key=api_key)

                completion_tokens = 4096
                if "8b" in model_name:
                    completion_tokens = 1024

                initial_stream = await client.chat.completions.create(
                    messages=[
                        {"role": "system", "content": system_prompt},
                        {"role": "user", "content": question},
                    ],
                    model=model_name,
                    max_tokens=completion_tokens,
                    stream=True,
                )

                async def stream_groq(stream_iter):
                    async for chunk in stream_iter:
                        content = chunk.choices[0].delta.content or ""
                        if content:
                            yield content

                return response_wrapper(stream_groq(initial_stream))

            # --- OPENAI SUPPORT ---
            elif model == "openai":
                api_key = await self.db.get_api_key("openai", user_email=user_email)
                if not api_key:
                    raise ValueError("OpenAI API key not found")

                client = AsyncOpenAI(api_key=api_key)

                initial_stream = await client.chat.completions.create(
                    messages=[
                        {"role": "system", "content": system_prompt},
                        {"role": "user", "content": question},
                    ],
                    model=model_name,
                    stream=True,
                )

                async def stream_openai(stream_iter):
                    async for chunk in stream_iter:
                        content = chunk.choices[0].delta.content or ""
                        if content:
                            yield content

                return response_wrapper(stream_openai(initial_stream))

            # --- CLAUDE SUPPORT ---
            elif model == "claude":
                api_key = await self.db.get_api_key("claude", user_email=user_email)
                if not api_key:
                    raise ValueError("Anthropic API key not found")

                client = AsyncAnthropic(api_key=api_key)

                initial_stream = await client.messages.create(
                    max_tokens=1024,
                    system=system_prompt,
                    messages=[{"role": "user", "content": question}],
                    model=model_name,
                    stream=True,
                )

                async def stream_claude(stream_iter):
                    try:
                        async for text in stream_iter.text_stream:
                            yield text
                    except Exception as e:
                        yield f"Error: {str(e)}"

                return response_wrapper(stream_claude(initial_stream))

            # --- GEMINI SUPPORT ---
            elif model == "gemini":
                # Prioritize Environment Variables over Database
                api_key = os.getenv("GEMINI_API_KEY") or os.getenv("GOOGLE_API_KEY")
                if not api_key:
                    api_key = await self.db.get_api_key("gemini", user_email=user_email)
                if not api_key:
                    raise ValueError("Gemini API key not found")

                generation_config = {
                    "temperature": 0.7,
                    "top_p": 0.95,
                    "top_k": 64,
                    "max_output_tokens": 8192,
                    "system_instruction": system_prompt,
                }

                async def stream_gemini():
                    try:
                        async for chunk_text in stream_content_text_async(
                            api_key=api_key,
                            model=model_name,
                            contents=question,
                            config=generation_config,
                        ):
                            yield chunk_text
                    except Exception as e:
                        logger.error(f"Gemini streaming error: {e}", exc_info=True)
                        yield f"\n\nError during Gemini response: {str(e)}"

                return response_wrapper(stream_gemini())

            else:
                raise ValueError(f"Unsupported chat model: {model}")

        except Exception as e:
            logger.error(f"Error in chat_about_meeting: {e}", exc_info=True)
            raise e

    async def stream_response(
        self,
        system_prompt: str,
        user_query: str,
        model: str,
        model_name: str,
        user_email: Optional[str] = None,
    ):
        """
        Generic streaming response handler for different LLM providers.
        """
        try:
            # --- OLLAMA SUPPORT ---
            # --- OLLAMA SUPPORT ---
            # if model == "ollama":
            #     ollama_host = os.getenv("OLLAMA_HOST", "http://localhost:11434")
            #     client = AsyncClient(host=ollama_host)
            #     message = {"role": "user", "content": user_query}
            #     system_message = {"role": "system", "content": system_prompt}

            #     async def stream_ollama():
            #         async for part in await client.chat(
            #             model=model_name,
            #             messages=[system_message, message],
            #             stream=True,
            #         ):
            #             content = part["message"]["content"]
            #             yield content

            #     return stream_ollama()

            # --- GROQ SUPPORT ---
            if model == "groq":
                api_key = await self.db.get_api_key("groq", user_email=user_email)
                if not api_key:
                    api_key = os.getenv("GROQ_API_KEY")
                if not api_key:
                    raise ValueError("Groq API key not found.")

                client = AsyncGroq(api_key=api_key)
                initial_stream = await client.chat.completions.create(
                    messages=[
                        {"role": "system", "content": system_prompt},
                        {"role": "user", "content": user_query},
                    ],
                    model=model_name,
                    stream=True,
                )

                async def stream_groq(stream_iter):
                    async for chunk in stream_iter:
                        content = chunk.choices[0].delta.content or ""
                        if content:
                            yield content

                return stream_groq(initial_stream)

            # --- OPENAI SUPPORT ---
            elif model == "openai":
                api_key = await self.db.get_api_key("openai", user_email=user_email)
                if not api_key:
                    raise ValueError("OpenAI API key not found")

                client = AsyncOpenAI(api_key=api_key)
                initial_stream = await client.chat.completions.create(
                    messages=[
                        {"role": "system", "content": system_prompt},
                        {"role": "user", "content": user_query},
                    ],
                    model=model_name,
                    stream=True,
                )

                async def stream_openai(stream_iter):
                    async for chunk in stream_iter:
                        content = chunk.choices[0].delta.content or ""
                        if content:
                            yield content

                return stream_openai(initial_stream)

            # --- CLAUDE SUPPORT ---
            elif model == "claude":
                api_key = await self.db.get_api_key("claude", user_email=user_email)
                if not api_key:
                    raise ValueError("Anthropic API key not found")

                client = AsyncAnthropic(api_key=api_key)
                initial_stream = await client.messages.create(
                    max_tokens=4096,
                    system=system_prompt,
                    messages=[{"role": "user", "content": user_query}],
                    model=model_name,
                    stream=True,
                )

                async def stream_claude(stream_iter):
                    async for text in stream_iter.text_stream:
                        yield text

                return stream_claude(initial_stream)

            # --- GEMINI SUPPORT ---
            elif model == "gemini":
                # Prioritize Environment Variables over Database
                api_key = os.getenv("GEMINI_API_KEY") or os.getenv("GOOGLE_API_KEY")
                if not api_key:
                    api_key = await self.db.get_api_key("gemini", user_email=user_email)
                if not api_key:
                    raise ValueError("Gemini API key not found")

                async def stream_gemini():
                    async for chunk_text in stream_content_text_async(
                        api_key=api_key,
                        model=model_name,
                        contents=user_query,
                        config={"system_instruction": system_prompt},
                    ):
                        yield chunk_text

                return stream_gemini()

            else:
                raise ValueError(f"Unsupported model: {model}")

        except Exception as e:
            logger.error(f"Error in stream_response: {e}", exc_info=True)
            raise e

    async def refine_notes(
        self,
        notes: str,
        instruction: str,
        transcript_context: str,
        model: str,
        model_name: str,
        user_email: Optional[str] = None,
    ):
        """
        Refine meeting notes based on user instruction and transcript context.
        """
        system_prompt = f"""You are an expert meeting notes editor.
Your task is to REFINE the Current Meeting Notes based strictly on the User Instruction and the provided Context (Transcript).

Context (Meeting Transcript):
---
{transcript_context[:30000]}
---

Guidelines:
1. You MUST start your response with a detailed bulleted list of changes made.
2. You MUST then output exactly: "|||SEPARATOR|||" (without quotes).
3. After the separator, provide the FULL updated notes content.
"""

        user_query = f"""Current Meeting Notes:
---
{notes}
---

User Instruction: {instruction}
"""

        return await self.stream_response(
            system_prompt, user_query, model, model_name, user_email
        )
