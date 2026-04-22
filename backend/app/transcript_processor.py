from pydantic import BaseModel
from typing import List, Tuple, Literal
from pydantic_ai import Agent
from pydantic_ai.models.anthropic import AnthropicModel
from pydantic_ai.models.groq import GroqModel
from pydantic_ai.models.openai import OpenAIModel
from pydantic_ai.providers.openai import OpenAIProvider
from pydantic_ai.providers.groq import GroqProvider
from pydantic_ai.providers.anthropic import AnthropicProvider

import logging
import os
from dotenv import load_dotenv
from db import DatabaseManager
from ollama import chat
import asyncio
from ollama import AsyncClient





# Set up logging
logging.basicConfig(
    level=logging.DEBUG,
    format='%(asctime)s - %(levelname)s - [%(filename)s:%(lineno)d] - %(message)s'
)
logger = logging.getLogger(__name__)

load_dotenv()  # Load environment variables from .env file

db = DatabaseManager()

class Block(BaseModel):
    """Represents a block of content in a section.
    
    Block types must align with frontend rendering capabilities:
    - 'text': Plain text content
    - 'bullet': Bulleted list item
    - 'heading1': Large section heading
    - 'heading2': Medium section heading
    
    Colors currently supported:
    - 'gray': Gray text color
    - '' or any other value: Default text color
    """
    id: str
    type: Literal['bullet', 'heading1', 'heading2', 'text']
    content: str
    color: str  # Frontend currently only uses 'gray' or default

class Section(BaseModel):
    """Represents a section in the meeting summary"""
    title: str
    blocks: List[Block]

class MeetingNotes(BaseModel):
    """Represents the meeting notes"""
    meeting_name: str
    sections: List[Section]

class People(BaseModel):
    """Represents the people in the meeting. Always have this part in the output. Title - Person Name (Role, Details)"""
    title: str
    blocks: List[Block]

class SummaryResponse(BaseModel):
    """Represents the meeting summary response based on a section of the transcript"""
    MeetingName : str
    People : People
    SessionSummary : Section
    CriticalDeadlines: Section
    KeyItemsDecisions: Section
    ImmediateActionItems: Section
    NextSteps: Section
    MeetingNotes: MeetingNotes

# --- Main Class Used by main.py ---

class TranscriptProcessor:
    """Handles the processing of meeting transcripts using AI models."""
    def __init__(self):
        """Initialize the transcript processor."""
        logger.info("TranscriptProcessor initialized.")
        self.db = DatabaseManager()
        self.active_clients = []  # Track active Ollama client sessions
    async def process_transcript(self, text: str, model: str, model_name: str, chunk_size: int = 5000, overlap: int = 1000, custom_prompt: str = "") -> Tuple[int, List[str]]:
        """
        Process transcript text into chunks and generate structured summaries for each chunk using an AI model.

        Args:
            text: The transcript text.
            model: The AI model provider ('claude', 'ollama', 'groq', 'openai').
            model_name: The specific model name.
            chunk_size: The size of each text chunk.
            overlap: The overlap between consecutive chunks.
            custom_prompt: A custom prompt to use for the AI model.

        Returns:
            A tuple containing:
            - The number of chunks processed.
            - A list of JSON strings, where each string is the summary of a chunk.
        """

        logger.info(f"Processing transcript (length {len(text)}) with model provider={model}, model_name={model_name}, chunk_size={chunk_size}, overlap={overlap}")

        all_json_data = []
        agent = None # Define agent variable
        llm = None # Define llm variable

        try:
            # Select and initialize the AI model and agent
            if model == "claude":
                api_key = await db.get_api_key("claude")
                if not api_key: raise ValueError("ANTHROPIC_API_KEY environment variable not set")
                llm = AnthropicModel(model_name, provider=AnthropicProvider(api_key=api_key))
                logger.info(f"Using Claude model: {model_name}")
            elif model == "ollama":
                # Use environment variable for Ollama host configuration
                ollama_host = os.getenv('OLLAMA_HOST', 'http://localhost:11434')
                ollama_base_url = f"{ollama_host}/v1"
                ollama_model = OpenAIModel(
                    model_name=model_name, provider=OpenAIProvider(base_url=ollama_base_url)
                )
                llm = ollama_model
                if model_name.lower().startswith("phi4") or model_name.lower().startswith("llama"):
                    chunk_size = 10000
                    overlap = 1000
                else:
                    chunk_size = 30000
                    overlap = 1000
                logger.info(f"Using Ollama model: {model_name}")
            elif model == "groq":
                api_key = await db.get_api_key("groq")
                if not api_key: raise ValueError("GROQ_API_KEY environment variable not set")
                llm = GroqModel(model_name, provider=GroqProvider(api_key=api_key))
                logger.info(f"Using Groq model: {model_name}")
            # --- ADD OPENAI SUPPORT HERE ---
            elif model == "openai":
                api_key = await db.get_api_key("openai")
                if not api_key: raise ValueError("OPENAI_API_KEY environment variable not set")
                llm = OpenAIModel(model_name, provider=OpenAIProvider(api_key=api_key))
                logger.info(f"Using OpenAI model: {model_name}")
            # --- END OPENAI SUPPORT ---
            else:
                logger.error(f"Unsupported model provider requested: {model}")
                raise ValueError(f"Unsupported model provider: {model}")

            # Initialize the agent with the selected LLM
            agent = Agent(
                llm,
                result_type=SummaryResponse,
                result_retries=2,
            )
            logger.info("Pydantic-AI Agent initialized.")

            # Split transcript into chunks
            step = chunk_size - overlap
            if step <= 0:
                logger.warning(f"Overlap ({overlap}) >= chunk_size ({chunk_size}). Adjusting overlap.")
                overlap = max(0, chunk_size - 100)
                step = chunk_size - overlap

            chunks = [text[i:i+chunk_size] for i in range(0, len(text), step)]
            num_chunks = len(chunks)
            logger.info(f"Split transcript into {num_chunks} chunks.")

            for i, chunk in enumerate(chunks):
                logger.info(f"Processing chunk {i+1}/{num_chunks}...")
                try:
                    # Run the agent to get the structured summary for the chunk
                    if model != "ollama":
                        summary_result = await agent.run(
                            f"""다음 회의 전사본 청크를 바탕으로, 요구되는 JSON 구조에 맞게 관련 정보를 추출하십시오. 특정 섹션(예: Critical Deadlines)에 해당하는 내용이 없을 경우, 해당 섹션의 'blocks'를 빈 목록으로 반환하십시오. 출력은 반드시 JSON 데이터만 포함해야 합니다.

                            중요: 블록 유형(Block types)은 반드시 다음 중 하나이어야 합니다: 'text', 'bullet', 'heading1', 'heading2'
                            - 'text': 일반 단락(regular paragraphs)에 사용
                            - 'bullet': 목록 항목(list items)에 사용
                            - 'heading1': 주요 제목(major headings)에 사용
                            - 'heading2': 소제목(subheadings)에 사용

                            color 필드에는 중요도가 낮은 내용에 'gray'를, 기본값에는 '' (빈 문자열)을 사용하십시오.

                            전사본 청크:
                            ---
                        {chunk}
                        ---

                        모든 관련 실행 항목(action items)을 빠짐없이 포함하십시오. 전사본에는 철자 오류가 있을 수 있으므로, 문맥을 고려하여 필요한 경우 수정하십시오.

                        요약을 생성할 때 다음 추가 맥락을 반영하십시오:
                        ---
                        {custom_prompt}
                        ---
                        출력은 반드시 JSON 데이터만 포함해야 합니다.
                        """,
                    )
                    else:
                        logger.info(f"Using Ollama model: {model_name} and chunk size: {chunk_size} with overlap: {overlap}")
                        response = await self.chat_ollama_model(model_name, chunk, custom_prompt)
                        
                        # Check if response is already a SummaryResponse object or a string that needs validation
                        if isinstance(response, SummaryResponse):
                            summary_result = response
                        else:
                            # If it's a string (JSON), validate it
                            summary_result = SummaryResponse.model_validate_json(response)
                            
                        logger.info(f"Summary result for chunk {i+1}: {summary_result}")
                        logger.info(f"Summary result type for chunk {i+1}: {type(summary_result)}")

                    if hasattr(summary_result, 'data') and isinstance(summary_result.data, SummaryResponse):
                         final_summary_pydantic = summary_result.data
                    elif isinstance(summary_result, SummaryResponse):
                         final_summary_pydantic = summary_result
                    else:
                         logger.error(f"Unexpected result type from agent for chunk {i+1}: {type(summary_result)}")
                         continue # Skip this chunk

                    # Convert the Pydantic model to a JSON string
                    chunk_summary_json = final_summary_pydantic.model_dump_json()
                    all_json_data.append(chunk_summary_json)
                    logger.info(f"Successfully generated summary for chunk {i+1}.")

                except Exception as chunk_error:
                    logger.error(f"Error processing chunk {i+1}: {chunk_error}", exc_info=True)

            logger.info(f"Finished processing all {num_chunks} chunks.")
            return num_chunks, all_json_data

        except Exception as e:
            logger.error(f"Error during transcript processing: {str(e)}", exc_info=True)
            raise
    
    async def chat_ollama_model(self, model_name: str, transcript: str, custom_prompt: str):
        message = {
        'role': 'system',
        'content': f'''
        다음 회의 전사본 청크를 바탕으로, 요구되는 JSON 구조에 맞게 관련 정보를 추출하십시오. 특정 섹션(예: Critical Deadlines)에 해당하는 내용이 없을 경우, 해당 섹션의 'blocks'를 빈 목록으로 반환하십시오. 출력은 반드시 JSON 데이터만 포함해야 합니다.

        전사본 청크:
            ---
            {transcript}
            ---
        모든 관련 실행 항목(action items)을 빠짐없이 포함하십시오. 전사본에는 철자 오류가 있을 수 있으므로, 문맥을 고려하여 필요한 경우 수정하십시오.

        요약을 생성할 때 다음 추가 맥락을 반영하십시오:
        ---
        {custom_prompt}
        ---

        출력은 반드시 JSON 데이터만 포함해야 합니다.

        ''',
        }

        # Create a client and track it for cleanup
        ollama_host = os.getenv('OLLAMA_HOST', 'http://127.0.0.1:11434')
        client = AsyncClient(host=ollama_host)
        self.active_clients.append(client)
        
        try:
            response = await client.chat(model=model_name, messages=[message], stream=True, format=SummaryResponse.model_json_schema())
            
            full_response = ""
            async for part in response:
                content = part['message']['content']
                print(content, end='', flush=True)
                full_response += content
            
            try:
                summary = SummaryResponse.model_validate_json(full_response)
                print("\n", summary.model_dump_json(indent=2), type(summary))
                return summary
            except Exception as e:
                print(f"\nError parsing response: {e}")
                return full_response
        except asyncio.CancelledError:
            logger.info("Ollama request was cancelled during shutdown")
            raise
        except Exception as e:
            logger.error(f"Error in Ollama chat: {e}")
            raise
        finally:
            # Remove the client from active clients list
            if client in self.active_clients:
                self.active_clients.remove(client)

    def cleanup(self):
        """Clean up resources used by the TranscriptProcessor."""
        logger.info("Cleaning up TranscriptProcessor resources")
        try:
            # Close database connections if any
            if hasattr(self, 'db') and self.db is not None:
                # self.db.close()
                logger.info("Database connection cleanup (using context managers)")
                
            # Cancel any active Ollama client sessions
            if hasattr(self, 'active_clients') and self.active_clients:
                logger.info(f"Terminating {len(self.active_clients)} active Ollama client sessions")
                for client in self.active_clients:
                    try:
                        # Close the client's underlying connection
                        if hasattr(client, '_client') and hasattr(client._client, 'close'):
                            asyncio.create_task(client._client.aclose())
                    except Exception as client_error:
                        logger.error(f"Error closing Ollama client: {client_error}", exc_info=True)
                # Clear the list
                self.active_clients.clear()
                logger.info("All Ollama client sessions terminated")
        except Exception as e:
            logger.error(f"Error during TranscriptProcessor cleanup: {str(e)}", exc_info=True)

        