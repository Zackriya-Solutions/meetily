//! Encoder + autoregressive greedy decoder for Cohere Transcribe ONNX.
//!
//! # Assumed ONNX contract
//!
//! The [onnx-community/cohere-transcribe-03-2026-ONNX](https://huggingface.co/onnx-community/cohere-transcribe-03-2026-ONNX)
//! export follows the Whisper-style `transformers` encoder–decoder interface:
//!
//! **Encoder** (`encoder_model_q4f16.onnx`):
//! * input  `input_features` : f32 [1, 80, 3000]
//! * output `last_hidden_state` : f32 [1, T_enc, d_model]
//!
//! **Decoder** (`decoder_model_merged_q4f16.onnx`, merged = with KV cache):
//! * input  `input_ids` : i64 [1, L]
//! * input  `encoder_hidden_states` : f32 [1, T_enc, d_model]
//! * input  `use_cache_branch` : bool [1]
//! * input  `past_key_values.N.{decoder,encoder}.{key,value}` : f32 [...]  (present but empty on first step)
//! * output `logits` : f32 [1, L, vocab_size]
//! * output `present.N...` : new KV cache tensors
//!
//! The first forward pass is run with `use_cache_branch = false` and the full
//! prompt (`[SOT, LANG, TRANSCRIBE, NO_TIMESTAMPS]`). Subsequent passes feed
//! only the last generated token with `use_cache_branch = true` and the KV
//! cache from the previous step.
//!
//! # Status
//!
//! The implementation below issues the encoder pass in full and runs the
//! decoder **without** KV reuse. This is slow (O(L²) attention per step) but
//! matches the non-cached branch, which is the portable lowest-common-denominator
//! signature for the merged export. A follow-up pass will wire up `use_cache_branch`
//! and the `past_key_values` plumbing once the exact per-layer input names are
//! confirmed against the downloaded model.

use anyhow::{anyhow, Result};
use log::debug;
use ndarray::{Array2, Array3, ArrayD, Axis};
use ort::inputs;
use ort::session::Session;
use ort::value::TensorRef;
use tokenizers::Tokenizer;

use super::tokenizer::{decode as decode_ids, resolve_special_tokens};

/// Run the encoder + autoregressive decoder. Returns the decoded string.
pub fn run_greedy_decode(
    encoder: &Session,
    decoder: &Session,
    mel: Array2<f32>,
    tokenizer: &Tokenizer,
    language: &str,
    max_new_tokens: usize,
) -> Result<String> {
    let specials = resolve_special_tokens(tokenizer, language)?;

    // Encoder expects [batch=1, n_mels=80, n_frames=3000].
    let encoder_input = mel.insert_axis(Axis(0)); // [1, 80, 3000]
    let enc_inputs = inputs![
        "input_features" => TensorRef::from_array_view(encoder_input.view().into_dyn())?,
    ];
    let enc_out = encoder.run(enc_inputs).map_err(|e| anyhow!("encoder run: {e}"))?;
    let encoder_hidden: ArrayD<f32> = enc_out
        .get("last_hidden_state")
        .ok_or_else(|| anyhow!("encoder missing 'last_hidden_state'"))?
        .try_extract_array::<f32>()
        .map_err(|e| anyhow!("extract encoder output: {e}"))?
        .to_owned();

    debug!(
        "cohere encoder output shape = {:?}",
        encoder_hidden.shape()
    );

    // Initial decoder prompt.
    let mut tokens: Vec<i64> = vec![
        specials.sot as i64,
        specials.lang as i64,
        specials.transcribe as i64,
        specials.no_timestamps as i64,
    ];
    let eot = specials.eot as i64;

    let max_total = tokens.len() + max_new_tokens;

    // Non-cached autoregressive decode. Every step re-runs the decoder over the
    // full prefix; this is simpler to wire up than the KV-cache variant.
    while tokens.len() < max_total {
        let input_ids = Array2::from_shape_vec((1, tokens.len()), tokens.clone())
            .map_err(|e| anyhow!("build input_ids: {e}"))?;

        // Represent the non-cache branch. The merged ONNX export accepts a
        // boolean gate; passing `false` here skips reading empty KV tensors.
        let use_cache = Array2::<bool>::from_elem((1, 1), false);

        let dec_inputs = inputs![
            "input_ids" => TensorRef::from_array_view(input_ids.view().into_dyn())?,
            "encoder_hidden_states" => TensorRef::from_array_view(encoder_hidden.view())?,
            "use_cache_branch" => TensorRef::from_array_view(use_cache.view().into_dyn())?,
        ];

        let dec_out = decoder
            .run(dec_inputs)
            .map_err(|e| anyhow!("decoder run: {e}"))?;

        let logits: ArrayD<f32> = dec_out
            .get("logits")
            .ok_or_else(|| anyhow!("decoder missing 'logits'"))?
            .try_extract_array::<f32>()
            .map_err(|e| anyhow!("extract logits: {e}"))?
            .to_owned();

        // `logits` is [1, L, vocab]. Pick the slice for the final position.
        let shape = logits.shape().to_vec();
        if shape.len() != 3 || shape[0] != 1 {
            return Err(anyhow!(
                "unexpected logits shape {:?}, expected [1, L, vocab]",
                shape
            ));
        }
        let last_pos = shape[1] - 1;
        let vocab = shape[2];
        let mut best_id = 0i64;
        let mut best_val = f32::NEG_INFINITY;
        for v in 0..vocab {
            let x = logits[[0, last_pos, v]];
            if x > best_val {
                best_val = x;
                best_id = v as i64;
            }
        }

        if best_id == eot {
            break;
        }
        tokens.push(best_id);
    }

    // Strip the four-token prompt before decoding back to text.
    let body: Vec<u32> = tokens.iter().skip(4).map(|&t| t as u32).collect();
    decode_ids(tokenizer, &body)
}

// Keep a reference to `Array3` so we can widen the decoder inputs later when
// KV caching is wired up without re-importing. Silences the unused-import check.
#[allow(dead_code)]
fn _touch_array3(_: Array3<f32>) {}
