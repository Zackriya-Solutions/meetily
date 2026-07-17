-- Add language and initial prompt configuration to transcript_settings
ALTER TABLE transcript_settings ADD COLUMN meeting_language TEXT NOT NULL DEFAULT 'pt';
ALTER TABLE transcript_settings ADD COLUMN whisper_initial_prompt TEXT NOT NULL DEFAULT 'A seguir, a transcrição de uma reunião. A transcrição deve ser precisa, com pontuação e capitalização corretas. Nomes próprios e siglas técnicas devem ser mantidos em maiúsculas quando apropriado.';
