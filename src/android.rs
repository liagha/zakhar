//! Android JNI bridge for the embedded zakhar core.
//!
//! Compiled only when the `jni` feature is enabled (see `Cargo.toml`). The
//! Kotlin `ZakharCore` module drives the conversation engine on-device. All
//! logic lives in `crate::mobile`; these functions only translate JNI values
//! in and out.

use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jstring};
use jni::JNIEnv;
use serde_json::json;

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_zakhar_mobile_ZakharCore_nativeStartChatSession(
    mut env: JNIEnv,
    _class: JClass,
    provider_json: JString,
    messages_json: JString,
    auto_approve: jboolean,
) -> jstring {
    let provider: Option<String> = env.get_string(&provider_json).ok().map(|s| s.into());
    let messages: Option<String> = env.get_string(&messages_json).ok().map(|s| s.into());
    let id = match (provider, messages) {
        (Some(p), Some(m)) if !p.trim().is_empty() => run(&mut env, &p, &m, auto_approve != 0),
        _ => respond_error(&mut env, "missing provider config or messages"),
    };
    id
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_zakhar_mobile_ZakharCore_nativePollEvents(
    mut env: JNIEnv,
    _class: JClass,
    session: JString,
    timeout_ms: i64,
) -> jstring {
    let id: String = env.get_string(&session).ok().map(|s| s.into()).unwrap_or_default();
    let payload = crate::mobile::poll(&id, timeout_ms);
    respond(&mut env, &payload)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_zakhar_mobile_ZakharCore_nativeResolveApproval(
    mut env: JNIEnv,
    _class: JClass,
    session: JString,
    approved: jboolean,
) -> jstring {
    let id: String = env.get_string(&session).ok().map(|s| s.into()).unwrap_or_default();
    let payload = crate::mobile::approve(&id, approved != 0);
    respond(&mut env, &payload)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_zakhar_mobile_ZakharCore_nativeCancelSession(
    mut env: JNIEnv,
    _class: JClass,
    session: JString,
) -> jstring {
    let id: String = env.get_string(&session).ok().map(|s| s.into()).unwrap_or_default();
    let payload = crate::mobile::cancel(&id);
    respond(&mut env, &payload)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_zakhar_mobile_ZakharCore_nativeDropSession(
    mut env: JNIEnv,
    _class: JClass,
    session: JString,
) -> jstring {
    let id: String = env.get_string(&session).ok().map(|s| s.into()).unwrap_or_default();
    let payload = crate::mobile::discard(&id);
    respond(&mut env, &payload)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_zakhar_mobile_ZakharCore_nativeContextKeys(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    respond(&mut env, &crate::mobile::keys())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_zakhar_mobile_ZakharCore_nativeRecentEvents(
    mut env: JNIEnv,
    _class: JClass,
    n: jni::sys::jint,
) -> jstring {
    respond(&mut env, &crate::mobile::recent(n as usize))
}

fn run(env: &mut JNIEnv, provider_json: &str, messages_json: &str, auto_approve: bool) -> jstring {
    let pcfg: crate::provider::types::Config = match serde_json::from_str(provider_json) {
        Ok(c) => c,
        Err(e) => return respond_error(env, &format!("bad provider config: {e}")),
    };
    if pcfg.base_url.trim().is_empty() {
        return respond_error(env, "missing provider base url");
    }
    let provider: Box<dyn crate::provider::Provider> =
        Box::new(crate::provider::openai::OpenAI::new("app", &pcfg));
    let id = crate::mobile::start(provider, messages_json, auto_approve);
    respond(env, &id)
}

fn respond(env: &mut JNIEnv, payload: &str) -> jstring {
    let output = env.new_string(payload).unwrap_or_else(|_| env.new_string("{}").unwrap());
    output.into_raw()
}

fn respond_error(env: &mut JNIEnv, msg: &str) -> jstring {
    respond(env, &json!({ "error": msg }).to_string())
}