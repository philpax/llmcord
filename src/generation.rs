use std::{collections::HashSet, num::NonZeroU32, thread::JoinHandle};

use llama_cpp_2::{
    context::params::LlamaContextParams,
    llama_backend::LlamaBackend,
    llama_batch::{BatchAddError, LlamaBatch},
    model::{LlamaChatMessage, LlamaModel, Special},
    sampling::LlamaSampler,
    ApplyChatTemplateError, ChatTemplateError, DecodeError, LlamaContextLoadError,
    NewLlamaChatMessageError, StringToTokenError, TokenToStringError,
};
use serenity::model::prelude::MessageId;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum InferenceError {
    #[error("The generation was cancelled.")]
    Cancelled,
    #[error("{0}")]
    Custom(String),
    #[error("{0}")]
    LlamaContextLoadError(#[from] LlamaContextLoadError),
    #[error("{0}")]
    ChatTemplateError(#[from] ChatTemplateError),
    #[error("{0}")]
    ApplyChatTemplateError(#[from] ApplyChatTemplateError),
    #[error("{0}")]
    NewLlamaChatMessageError(#[from] NewLlamaChatMessageError),
    #[error("{0}")]
    StringToTokenError(#[from] StringToTokenError),
    #[error("{0}")]
    DecodeError(#[from] DecodeError),
    #[error("{0}")]
    TokenToStringError(#[from] TokenToStringError),
    #[error("{0}")]
    BatchAddError(#[from] BatchAddError),
}
impl InferenceError {
    pub fn custom(s: impl Into<String>) -> Self {
        Self::Custom(s.into())
    }
}

pub struct Request {
    pub system_prompt: String,
    pub user_prompt: String,
    pub token_tx: flume::Sender<Token>,
    pub message_id: MessageId,
    pub seed: Option<u32>,
}

pub enum Token {
    Token(String),
    Error(InferenceError),
}

pub fn make_thread(
    backend: LlamaBackend,
    model: LlamaModel,
    ctx_len: u32,
    request_rx: flume::Receiver<Request>,
    cancel_rx: flume::Receiver<MessageId>,
) -> JoinHandle<()> {
    std::thread::spawn(move || loop {
        if let Ok(request) = request_rx.try_recv() {
            match process_incoming_request(&request, &backend, &model, ctx_len, &cancel_rx) {
                Ok(_) => {}
                Err(e) => {
                    if let Err(err) = request.token_tx.send(Token::Error(e)) {
                        eprintln!("Failed to send error: {err:?}");
                    }
                }
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(5));
    })
}

fn process_incoming_request(
    request: &Request,
    backend: &LlamaBackend,
    model: &LlamaModel,
    ctx_len: u32,
    cancel_rx: &flume::Receiver<MessageId>,
) -> Result<(), InferenceError> {
    let mut ctx = model.new_context(
        backend,
        LlamaContextParams::default().with_n_ctx(NonZeroU32::new(ctx_len)),
    )?;
    let template = model.get_chat_template()?;
    let message = model.apply_chat_template(
        &template,
        &[
            LlamaChatMessage::new("system".into(), request.system_prompt.clone())?,
            LlamaChatMessage::new("user".into(), request.user_prompt.clone())?,
        ],
        true,
    )?;
    let tokens_list = model.str_to_token(&message, llama_cpp_2::model::AddBos::Always)?;

    let mut batch = LlamaBatch::new(512, 1);

    let last_index = tokens_list.len() as i32 - 1;
    for (i, token) in (0_i32..).zip(tokens_list.into_iter()) {
        // llama_decode will output logits only for the last token of the prompt
        let is_last = i == last_index;
        batch.add(token, i, &[0], is_last).unwrap();
    }
    ctx.decode(&mut batch)?;

    let mut n_cur = batch.n_tokens();

    let mut decoder = encoding_rs::UTF_8.new_decoder();
    let mut sampler = LlamaSampler::chain_simple([
        LlamaSampler::dist(request.seed.unwrap_or(1234)),
        LlamaSampler::greedy(),
    ]);

    while n_cur <= ctx_len as i32 {
        let cancellation_requests: HashSet<_> = cancel_rx.drain().collect();
        if cancellation_requests.contains(&request.message_id) {
            return Err(InferenceError::Cancelled);
        }

        // sample the next token
        {
            let token = sampler.sample(&ctx, batch.n_tokens() - 1);

            sampler.accept(token);

            // is it an end of stream?
            if model.is_eog_token(token) {
                eprintln!();
                break;
            }

            let output_bytes = model.token_to_bytes(token, Special::Tokenize)?;
            // use `Decoder.decode_to_string()` to avoid the intermediate buffer
            let mut output_string = String::with_capacity(32);
            let _decode_result = decoder.decode_to_string(&output_bytes, &mut output_string, false);
            request
                .token_tx
                .send(Token::Token(output_string))
                .map_err(|_| InferenceError::custom("Failed to send token to channel."))?;

            batch.clear();
            batch.add(token, n_cur, &[0], true)?;
        }

        n_cur += 1;

        ctx.decode(&mut batch)?;
    }

    Ok(())
}
