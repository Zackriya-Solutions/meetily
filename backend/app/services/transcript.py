import logging
import os
import asyncio
from typing import List, Tuple, Optional
from dotenv import load_dotenv

from pydantic_ai import Agent
from pydantic_ai.models.anthropic import AnthropicModel
from pydantic_ai.models.groq import GroqModel
from pydantic_ai.models.openai import OpenAIModel
from pydantic_ai.models.gemini import GeminiModel
from pydantic_ai.providers.openai import OpenAIProvider
from pydantic_ai.providers.groq import GroqProvider
from pydantic_ai.providers.anthropic import AnthropicProvider
from pydantic_ai.providers.google_gla import GoogleGLAProvider

# from ollama import AsyncClient

try:
    from ..db import DatabaseManager
    from ..schemas.summary import SummaryResponse
    from .gemini_client import generate_content_text_async
except (ImportError, ValueError):
    from db import DatabaseManager
    from schemas.summary import SummaryResponse
    from services.gemini_client import generate_content_text_async

# Set up logging
logger = logging.getLogger(__name__)

load_dotenv()


class TranscriptService:
    """Handles the processing of meeting transcripts using AI models."""

    def __init__(self, db: DatabaseManager):
        """Initialize the transcript processor."""
        logger.info("TranscriptService initialized.")
        self.db = db
        self.active_clients = []  # Track active Ollama client sessions

    def _clean_json(self, text: str) -> str:
        """Clean markdown formatting from JSON string."""
        text = text.strip()
        # Try to find JSON block with regex first (handles preamble text)
        import re

        match = re.search(r"```(?:json)?\s*(\{.*?\})\s*```", text, re.DOTALL)
        if match:
            return match.group(1)

        # Fallback: simple strip if no code blocks but just text
        if text.startswith("```json"):
            text = text[7:]
        elif text.startswith("```"):
            text = text[3:]
        if text.endswith("```"):
            text = text[:-3]
        return text.strip()

    def _clean_transcript_text(self, text: str) -> str:
        """
        Clean common frontend artifacts from transcript text.
        Specifically removes 'undefined' prefixes caused by JS interpolation bugs.
        """
        if not text:
            return ""

        # Remove 'undefined' if it appears at the start of lines or text
        cleaned = text.replace("undefined ", "")

        # Also fix specific patterns seen in logs
        cleaned = cleaned.replace("**Speaker 0:** undefined", "**Speaker 0:**")
        cleaned = cleaned.replace("**Unknown:** undefined", "**Unknown:**")

        return cleaned

    async def process_transcript(
        self,
        text: str,
        model: str,
        model_name: str,
        chunk_size: int = 5000,
        overlap: int = 1000,
        custom_prompt: str = "",
        user_email: Optional[str] = None,
    ) -> Tuple[int, List[str]]:
        """
        Process transcript text into chunks and generate structured summaries for each chunk using an AI model.
        """
        # CLEANUP: Remove artifacts before processing
        text = self._clean_transcript_text(text)

        logger.info(
            f"Processing transcript (length {len(text)}) with model provider={model}, model_name={model_name}, chunk_size={chunk_size}, overlap={overlap}"
        )

        all_json_data = []
        agent = None
        llm = None

        try:
            # Select and initialize the AI model and agent
            if model == "claude":
                api_key = await self.db.get_api_key("claude", user_email=user_email)
                if not api_key:
                    raise ValueError("ANTHROPIC_API_KEY environment variable not set")
                llm = AnthropicModel(
                    model_name, provider=AnthropicProvider(api_key=api_key)
                )
                logger.info(f"Using Claude model: {model_name}")
            # elif model == "ollama":
            #     # Use environment variable for Ollama host configuration
            #     ollama_host = os.getenv("OLLAMA_HOST", "http://localhost:11434")
            #     ollama_base_url = f"{ollama_host}/v1"
            #     ollama_model = OpenAIModel(
            #         model_name=model_name,
            #         provider=OpenAIProvider(base_url=ollama_base_url),
            #     )
            #     llm = ollama_model
            #     if model_name.lower().startswith(
            #         "phi4"
            #     ) or model_name.lower().startswith("llama"):
            #         chunk_size = 10000
            #         overlap = 1000
            #     else:
            #         chunk_size = 30000
            #         overlap = 1000
            #     logger.info(f"Using Ollama model: {model_name}")
            elif model == "groq":
                api_key = await self.db.get_api_key("groq", user_email=user_email)
                if not api_key:
                    raise ValueError("GROQ_API_KEY environment variable not set")
                llm = GroqModel(model_name, provider=GroqProvider(api_key=api_key))
                logger.info(f"Using Groq model: {model_name}")
            elif model == "openai":
                api_key = await self.db.get_api_key("openai", user_email=user_email)
                if not api_key:
                    raise ValueError("OPENAI_API_KEY environment variable not set")
                llm = OpenAIModel(model_name, provider=OpenAIProvider(api_key=api_key))
                logger.info(f"Using OpenAI model: {model_name}")
            elif model == "gemini":
                # Prioritize Environment Variables over Database
                api_key = os.getenv("GEMINI_API_KEY") or os.getenv("GOOGLE_API_KEY")
                if api_key:
                    logger.info("Using Gemini API key from environment variable")
                else:
                    api_key = await self.db.get_api_key("gemini", user_email=user_email)
                    if api_key:
                        logger.info(f"Using Gemini API key from database for user: {user_email}")
                
                if not api_key:
                    raise ValueError(
                        "Gemini API key not found. Set GEMINI_API_KEY environment variable."
                    )

                # Try simple client initialization to check for immediate key errors
                llm = GeminiModel(model_name, provider=GoogleGLAProvider(api_key=api_key))
                logger.info(f"Using Gemini model: {model_name}")
            else:
                logger.error(f"Unsupported model provider requested: {model}")
                raise ValueError(f"Unsupported model provider: {model}")

            # Initialize the agent with the selected LLM (for non-Gemini models)
            if model != "gemini":
                agent = Agent(
                    llm,
                    result_type=SummaryResponse,
                    result_retries=2,
                )
                logger.info(f"Pydantic-AI Agent initialized for {model}.")

            # Split transcript into chunks
            step = chunk_size - overlap
            if step <= 0:
                logger.warning(
                    f"Overlap ({overlap}) >= chunk_size ({chunk_size}). Adjusting overlap."
                )
                overlap = max(0, chunk_size - 100)
                step = chunk_size - overlap

            chunks = [text[i : i + chunk_size] for i in range(0, len(text), step)]
            num_chunks = len(chunks)
            logger.info(f"Split transcript into {num_chunks} chunks.")

            for i, chunk in enumerate(chunks):
                logger.info(f"Processing chunk {i + 1}/{num_chunks}...")

                # Retry logic for rate limits
                max_retries = 3
                retry_delay = 5
                success = False

                for attempt in range(max_retries):
                    try:
                        # Run the agent or Gemini to get the structured summary for the chunk
                        if model == "gemini":
                            # Get schema for prompt context to ensure strict structure
                            import json

                            schema_desc = json.dumps(
                                SummaryResponse.model_json_schema(), indent=2
                            )

                            prompt = f"""Given the following meeting transcript chunk, extract the relevant information according to the required JSON structure.
                            
                            You must output a JSON object that strictly adheres to the following schema:
                            {schema_desc}

                            ONE-SHOT EXAMPLE FOR YOUR GUIDANCE:
                            If the transcript was: "Speaker 0: Rahul will work on backend. Priya will handle frontend."
                            Your output should look like this:
                            {{
                                "MeetingName": "Development Sync",
                                "People": {{ "present": ["Rahul", "Priya"], "absent": [] }},
                                "SessionSummary": {{
                                    "title": "Summary",
                                    "blocks": [
                                        {{ "id": "1", "type": "text", "content": "The team discussed backend and frontend assignments.", "color": "" }}
                                    ]
                                }},
                                "ImmediateActionItems": {{
                                    "title": "Action Items",
                                    "blocks": [
                                        {{ "id": "2", "type": "bullet", "content": "Rahul: Work on backend architecture", "color": "blue" }},
                                        {{ "id": "3", "type": "bullet", "content": "Priya: Handle frontend integration", "color": "blue" }}
                                    ]
                                }},
                                "MeetingNotes": {{
                                    "sections": [
                                        {{
                                            "title": "Task Allocation",
                                            "blocks": [
                                                {{ "id": "4", "type": "bullet", "content": "Backend task assigned to Rahul.", "color": "" }},
                                                {{ "id": "5", "type": "bullet", "content": "Frontend task assigned to Priya.", "color": "" }}
                                            ]
                                        }}
                                    ]
                                }}
                            }}

                            IMPORTANT GUIDELINES:
                            1. Return ONLY valid JSON.
                            2. NEVER return empty 'blocks' arrays if there is relevant information in the transcript.
                            3. For 'MeetingNotes', organize content into logical 'sections', each with a 'title' and a list of 'blocks'.
                            4. 'blocks' must have 'type' (bullet, text, heading1, heading2) and 'content'.
                            5. Extract specific actionable items for 'ImmediateActionItems'. 
                            6. If someone is mentioned as doing something, it MUST be in 'ImmediateActionItems'.
                            7. 'MeetingName' should be a concise title for the meeting.
                            8. Capture ALL relevant points. Do not omit information.
                            
                            Transcript Chunk:
                            ---
                            {chunk}
                            ---
                            
                            Context/Instructions:
                            ---
                            {custom_prompt}
                            ---
                            """
                            # Use Gemini for structured output
                            response_text = await generate_content_text_async(
                                api_key=api_key,
                                model=model_name,
                                contents=prompt,
                                config={"response_mime_type": "application/json"},
                            )
                            chunk_summary_json = self._clean_json(response_text)
                            all_json_data.append(chunk_summary_json)
                            logger.info(
                                f"Successfully generated Gemini summary for chunk {i + 1}."
                            )
                            success = True
                            break

                        else: # Changed from elif model != "ollama": because Ollama support is removed
                            summary_result = await agent.run(
                                f"""Given the following meeting transcript chunk, extract the relevant information according to the required JSON structure. If a specific section (like Critical Deadlines) has no relevant information in this chunk, return an empty list for its 'blocks'. Ensure the output is only the JSON data.

                                IMPORTANT: Block types must be one of: 'text', 'bullet', 'heading1', 'heading2'
                                - Use 'text' for regular paragraphs
                                - Use 'bullet' for list items
                                - Use 'heading1' for major headings
                                - Use 'heading2' for subheadings
                                
                                For the color field, use 'gray' for less important content or '' (empty string) for default.

                                Transcript Chunk:
                                ---
                            {chunk}
                            ---

                            Please capture all relevant action items. Transcription can have spelling mistakes. correct it if required. context is important.
                            
                            While generating the summary, please add the following context:
                            ---
                            {custom_prompt}
                            ---
                            Make sure the output is only the JSON data.
                            """,
                            )
                        # else:
                        #     logger.info(
                        #         f"Using Ollama model: {model_name} and chunk size: {chunk_size} with overlap: {overlap}"
                        #     )
                        #     # Helper method included in this class
                        #     response = await self._chat_ollama_model(
                        #         model_name, chunk, custom_prompt
                        #     )

                        #     # Check if response is already a SummaryResponse object or a string that needs validation
                        #     if isinstance(response, SummaryResponse):
                        #         summary_result = response
                        #     else:
                        #         # If it's a string (JSON), validate it
                        #         summary_result = SummaryResponse.model_validate_json(
                        #             response
                        #         )

                        #     logger.info(
                        #         f"Summary result for chunk {i + 1}: {summary_result}"
                        #     )

                        if hasattr(summary_result, "data") and isinstance(
                            summary_result.data, SummaryResponse
                        ):
                            final_summary_pydantic = summary_result.data
                        elif isinstance(summary_result, SummaryResponse):
                            final_summary_pydantic = summary_result
                        else:
                            logger.error(
                                f"Unexpected result type from agent for chunk {i + 1}: {type(summary_result)}"
                            )
                            success = False  # Should likely count as fail
                            break  # Skip this chunk if type is wrong

                        # Convert the Pydantic model to a JSON string
                        chunk_summary_json = final_summary_pydantic.model_dump_json()
                        all_json_data.append(chunk_summary_json)
                        logger.info(
                            f"Successfully generated summary for chunk {i + 1}."
                        )
                        success = True
                        break

                    except Exception as chunk_error:
                        # Check for rate limits (429) or overloading
                        err_str = str(chunk_error).lower()
                        is_rate_limit = (
                            "429" in err_str
                            or "exhausted" in err_str
                            or "rate limit" in err_str
                        )

                        if is_rate_limit and attempt < max_retries - 1:
                            logger.warning(
                                f"Rate limit hit for chunk {i + 1} (attempt {attempt + 1}). Retrying in {retry_delay}s..."
                            )
                            await asyncio.sleep(retry_delay)
                            retry_delay *= 2
                            continue
                        else:
                            logger.error(
                                f"Error processing chunk {i + 1} (attempt {attempt + 1}): {chunk_error}",
                                exc_info=True,
                            )
                            # If it's the last attempt, we let it fail for this chunk
                            if attempt == max_retries - 1:
                                break

                if not success:
                    logger.warning(f"Skipping chunk {i + 1} after failures.")

                # Proactive rate limiting delay between chunks
                await asyncio.sleep(2)

            logger.info(f"Finished processing all {num_chunks} chunks.")
            return num_chunks, all_json_data

        except Exception as e:
            logger.error(f"Error during transcript processing: {str(e)}", exc_info=True)
            raise

    # async def _chat_ollama_model(
    #     self, model_name: str, transcript: str, custom_prompt: str
    # ):
    #     """Internal helper for Ollama summarization"""
    #     message = {
    #         "role": "system",
    #         "content": f"""
    #     Given the following meeting transcript chunk, extract the relevant information according to the required JSON structure. If a specific section (like Critical Deadlines) has no relevant information in this chunk, return an empty list for its 'blocks'. Ensure the output is only the JSON data.

    #     Transcript Chunk:
    #         ---
    #         {{transcript}}
    #         ---
    #     Please capture all relevant action items. Transcription can have spelling mistakes. correct it if required. context is important.
    #     
    #     While generating the summary, please add the following context:
    #     ---
    #     {{custom_prompt}}
    #     ---

    #     Make sure the output is only the JSON data.
    # 
    #     """,
    #     }

    #     # Create a client and track it for cleanup
    #     ollama_host = os.getenv("OLLAMA_HOST", "http://127.0.0.1:11434")

    #     # Use async context manager to ensure client is closed
    #     async with AsyncClient(host=ollama_host) as client:
    #         self.active_clients.append(client)
    #         try:
    #             # Increase timeout for long context processing
    #             response = await client.chat(
    #                 model=model_name,
    #                 messages=[message],
    #                 stream=True,
    #                 format=SummaryResponse.model_json_schema(),
    #             )

    #             full_response = ""
    #             async for part in response:
    #                 content = part["message"]["content"]
    #                 full_response += content

    #             try:
    #                 summary = SummaryResponse.model_validate_json(full_response)
    #                 return summary
    #             except Exception as e:
    #                 logger.error(
    #                     f"Error parsing Ollama response: {{e}}. Raw response: {{full_response[:200]}}..."
    #                 )
    #                 return full_response

    #         except asyncio.CancelledError:
    #             logger.info("Ollama request was cancelled during shutdown")
    #             raise
    #         except Exception as e:
    #             logger.error(f"Error in Ollama chat: {{e}}")
    #             raise
    #         finally:
    #             if client in self.active_clients:
    #                 self.active_clients.remove(client)

    def cleanup(self):
        """Clean up resources used by the TranscriptProcessor."""
        logger.info("Cleaning up TranscriptService resources")
        # try:
        #     # Cancel any active Ollama client sessions
        #     if hasattr(self, "active_clients") and self.active_clients:
        #         logger.info(
        #             f"Terminating {len(self.active_clients)} active Ollama client sessions"
        #         )
        #         for client in self.active_clients:
        #             try:
        #                 if hasattr(client, "_client") and hasattr(
        #                     client._client, "close"
        #                 ):
        #                     asyncio.create_task(client._client.aclose())
        #             except Exception as client_error:
        #                 logger.error(
        #                     f"Error closing Ollama client: {client_error}",
        #                     exc_info=True,
        #                 )
        #         self.active_clients.clear()
        # except Exception as e:
        #     logger.error(
        #         f"Error during TranscriptService cleanup: {str(e)}", exc_info=True
        #     )
