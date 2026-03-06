import logging

try:
    from ..db import DatabaseManager
    from .transcript import TranscriptService
except (ImportError, ValueError):
    from db import DatabaseManager
    from services.transcript import TranscriptService

logger = logging.getLogger(__name__)


class SummarizationService:
    """Handles the high-level coordination of summary generation."""

    def __init__(self):
        try:
            self.db = DatabaseManager()
            logger.info("Initializing SummarizationService components")
            self.transcript_service = TranscriptService(self.db)
            logger.info("SummarizationService initialized successfully")
        except Exception as e:
            logger.error(
                f"Failed to initialize SummarizationService: {str(e)}", exc_info=True
            )
            raise

    async def process_transcript(
        self,
        text: str,
        model: str = "gemini",
        model_name: str = "gemini-3-pro-preview",
        chunk_size: int = 5000,
        overlap: int = 1000,
        custom_prompt: str = "Generate a summary of the meeting transcript.",
        user_email: str = None,
    ) -> tuple:
        """Process a transcript text"""
        try:
            if not text:
                raise ValueError("Empty transcript text provided")

            # Validate chunk_size and overlap
            if chunk_size <= 0:
                raise ValueError("chunk_size must be positive")
            if overlap < 0:
                raise ValueError("overlap must be non-negative")
            if overlap >= chunk_size:
                overlap = chunk_size - 1  # Ensure overlap is less than chunk_size

            # Ensure step size is positive
            step_size = chunk_size - overlap
            if step_size <= 0:
                chunk_size = overlap + 1  # Adjust chunk_size to ensure positive step

            logger.info(
                f"Processing transcript of length {len(text)} with chunk_size={chunk_size}, overlap={overlap}"
            )

            # Delegate to TranscriptService
            result = await self.transcript_service.process_transcript(
                text=text,
                model=model,
                model_name=model_name,
                chunk_size=chunk_size,
                overlap=overlap,
                custom_prompt=custom_prompt,
                user_email=user_email,
            )

            num_chunks, all_json_data = result
            logger.info(f"Successfully processed transcript into {num_chunks} chunks")

            return num_chunks, all_json_data
        except Exception as e:
            logger.error(f"Error processing transcript: {str(e)}", exc_info=True)
            raise

    def cleanup(self):
        """Cleanup resources"""
        try:
            logger.info("Cleaning up resources")
            if hasattr(self, "transcript_service"):
                self.transcript_service.cleanup()
            logger.info("Cleanup completed successfully")
        except Exception as e:
            logger.error(f"Error during cleanup: {str(e)}", exc_info=True)
