// Copyright (c) 2023 Alibaba Group Holding Ltd.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use higress_wasm_rust::cluster_wrapper::FQDNCluster;
use higress_wasm_rust::log::Log;
use higress_wasm_rust::plugin_wrapper::{HttpContextWrapper, RootContextWrapper};
use higress_wasm_rust::request_wrapper::has_request_body;
use higress_wasm_rust::rule_matcher::{on_configure, RuleMatcher, SharedRuleMatcher};
use http::Method;
use jsonpath_rust::{JsonPath, JsonPathValue};
use multimap::MultiMap;
use proxy_wasm::traits::{Context, HttpContext, RootContext};
use proxy_wasm::types::{Bytes, ContextType, DataAction, HeaderAction, LogLevel};
use serde::de::Error;
use serde::Deserializer;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::ops::DerefMut;
use std::rc::{Rc, Weak};
use std::str::FromStr;
use std::time::Duration;

proxy_wasm::main! {{
    proxy_wasm::set_log_level(LogLevel::Trace);
    proxy_wasm::set_root_context(|_|Box::new(AiIntentRoot::new()));
}}

const PLUGIN_NAME: &str = "ai-intent";

#[derive(Default, Debug, Deserialize, Clone)]
struct AiIntentConfig {
    #[serde(default = "prompt_default")]
    prompt: String,
    categories: Vec<Category>,
    llm: LLMInfo,
    key_from: KVExtractor,
}

#[derive(Default, Debug, Deserialize, Serialize, Clone)]
struct Category {
    use_for: String,
    options: Vec<String>,
}

#[derive(Default, Debug, Deserialize, Clone)]
struct LLMInfo {
    proxy_service_name: String,
    proxy_url: String,
    #[serde(default = "proxy_model_default")]
    proxy_model: String,
    proxy_port: u16,
    #[serde(default)]
    proxy_domain: String,
    #[serde(default = "proxy_timeout_default")]
    proxy_timeout: u64,
    proxy_api_key: String,
    #[serde(skip)]
    _cluster: Option<FQDNCluster>,
}

impl LLMInfo {
    fn cluster(&self) -> FQDNCluster {
        FQDNCluster::new(
            &self.proxy_service_name,
            &self.proxy_domain,
            self.proxy_port,
        )
    }
}

impl AiIntentConfig {
    fn get_prompt(&self, message: &str) -> String {
        let prompt = self.prompt.clone();
        if let Ok(c) = serde_yaml::to_string(&self.categories) {
            prompt.replace("${categories}", &c)
        } else {
            prompt
        }
        .replace("${question}", message)
    }
}

#[derive(Debug, Deserialize, Clone)]
struct KVExtractor {
    #[serde(
        default = "request_body_default",
        deserialize_with = "deserialize_jsonpath"
    )]
    request_body: JsonPath,
    #[serde(
        default = "response_body_default",
        deserialize_with = "deserialize_jsonpath"
    )]
    response_body: JsonPath,
}

impl Default for KVExtractor {
    fn default() -> Self {
        Self {
            request_body: request_body_default(),
            response_body: response_body_default(),
        }
    }
}

fn prompt_default() -> String {
    r#"
You are an intelligent category recognition assistant, responsible for determining which preset category a question belongs to based on the user's query and predefined categories, and providing the corresponding category. 
The user's question is: '${question}'
The preset categories are: 
${categories}

Please respond directly with the category in the following manner:
- {"useFor": "scene1", "result": "result1"}
- {"useFor": "scene2", result: "result2"}
Ensure that different `useFor` are on different lines, and that `useFor` and `result` appear on the same line.
"#.to_string()
}

fn proxy_model_default() -> String {
    "qwen-long".to_string()
}

fn proxy_timeout_default() -> u64 {
    10_000
}

fn request_body_default() -> JsonPath {
    JsonPath::from_str("messages.@reverse.0.content").unwrap()
}

fn response_body_default() -> JsonPath {
    JsonPath::from_str("choices.0.message.content").unwrap()
}

fn deserialize_jsonpath<'de, D>(deserializer: D) -> Result<JsonPath, D::Error>
where
    D: Deserializer<'de>,
{
    let value: String = Deserialize::deserialize(deserializer)?;
    match JsonPath::from_str(&value) {
        Ok(jp) => Ok(jp),
        Err(_) => Err(Error::custom(format!("jsonpath error value {}", value))),
    }
}

fn get_message(body: &Bytes, json_path: &JsonPath) -> Option<String> {
    if let Ok(body) = String::from_utf8(body.clone()) {
        if let Ok(r) = serde_json::from_str(body.as_str()) {
            let json: Value = r;
            for v in json_path.find_slice(&json) {
                if let JsonPathValue::Slice(d, _) = v {
                    return d.as_str().map(|x| x.to_string());
                }
            }
        }
    }
    None
}

struct AiIntentRoot {
    log: Log,
    rule_matcher: SharedRuleMatcher<AiIntentConfig>,
}

impl AiIntentRoot {
    fn new() -> Self {
        let log = Log::new(PLUGIN_NAME.to_string());

        AiIntentRoot {
            log,
            rule_matcher: Rc::new(RefCell::new(RuleMatcher::default())),
        }
    }
}

impl Context for AiIntentRoot {}

impl RootContext for AiIntentRoot {
    fn on_configure(&mut self, plugin_configuration_size: usize) -> bool {
        on_configure(
            self,
            plugin_configuration_size,
            self.rule_matcher.borrow_mut().deref_mut(),
            &self.log,
        )
    }

    fn create_http_context(&self, context_id: u32) -> Option<Box<dyn HttpContext>> {
        self.create_http_context_use_wrapper(context_id)
    }

    fn get_type(&self) -> Option<ContextType> {
        Some(ContextType::HttpContext)
    }
}

impl RootContextWrapper<AiIntentConfig> for AiIntentRoot {
    fn rule_matcher(&self) -> &SharedRuleMatcher<AiIntentConfig> {
        &self.rule_matcher
    }

    fn create_http_context_wrapper(
        &self,
        _context_id: u32,
    ) -> Option<Box<dyn HttpContextWrapper<AiIntentConfig>>> {
        Some(Box::new(AiIntent {
            config: None,
            weak: Weak::default(),
            log: Log::new(PLUGIN_NAME.to_string()),
        }))
    }
}

struct AiIntent {
    config: Option<Rc<AiIntentConfig>>,
    log: Log,
    weak: Weak<RefCell<Box<dyn HttpContextWrapper<AiIntentConfig>>>>,
}

impl Context for AiIntent {}

impl HttpContext for AiIntent {
    fn on_http_request_headers(
        &mut self,
        _num_headers: usize,
        _end_of_stream: bool,
    ) -> HeaderAction {
        if has_request_body() {
            HeaderAction::StopIteration
        } else {
            HeaderAction::Continue
        }
    }
}

impl AiIntent {
    fn parse_intent(
        &self,
        status_code: u16,
        _headers: &MultiMap<String, String>,
        body: Option<Vec<u8>>,
    ) {
        self.log
            .infof(format_args!("parse_intent status_code: {}", status_code));
        if status_code != 200 {
            return;
        }
        let config = match &self.config {
            Some(c) => c,
            None => return,
        };
        if let Some(b) = body {
            if let Some(category) = get_message(&b, &config.key_from.response_body) {
                self.log.infof(format_args!(
                    "parse_intent response category is: : {}",
                    category
                ));
                for _cate in &config.categories {
                    todo!()
                }
            }
        }
    }
    fn http_call_intent(&mut self, config: &AiIntentConfig, message: &str) -> bool {
        self.log
            .infof(format_args!("original_question is:{}", message));
        let self_rc = match self.weak.upgrade() {
            Some(rc) => rc.clone(),
            None => return false,
        };
        let mut headers = MultiMap::new();
        headers.insert("Content-Type".to_string(), "application/json".to_string());
        headers.insert(
            "Authorization".to_string(),
            format!("Bearer {}", config.llm.proxy_api_key),
        );
        let prompt = config.get_prompt(message);
        self.log.infof(format_args!("after prompt is:{}", prompt));
        let proxy_request_body = json!({
            "model": config.llm.proxy_model,
            "messages": [
                {"role": "user", "content": prompt}
            ]
        })
        .to_string();
        self.log
            .infof(format_args!("proxy_url is:{}", config.llm.proxy_url));
        self.log
            .infof(format_args!("proxy_request_body is:{}", proxy_request_body));
        self.http_call(
            &config.llm.cluster(),
            &Method::POST,
            &config.llm.proxy_url,
            headers,
            Some(proxy_request_body.as_bytes()),
            Box::new(move |status_code, headers, body| {
                if let Some(this) = self_rc.borrow_mut().downcast_mut::<AiIntent>() {
                    this.parse_intent(status_code, headers, body);
                }
                self_rc.borrow().resume_http_request();
            }),
            Duration::from_millis(config.llm.proxy_timeout),
        )
        .is_ok()
    }
}

impl HttpContextWrapper<AiIntentConfig> for AiIntent {
    fn log(&self) -> &Log {
        &self.log
    }

    fn init_self_weak(
        &mut self,
        self_weak: Weak<RefCell<Box<dyn HttpContextWrapper<AiIntentConfig>>>>,
    ) {
        self.weak = self_weak
    }

    fn on_config(&mut self, config: Rc<AiIntentConfig>) {
        self.config = Some(config)
    }

    fn cache_request_body(&self) -> bool {
        true
    }

    fn on_http_request_complete_body(&mut self, req_body: &Bytes) -> DataAction {
        self.log
            .debug("start on_http_request_complete_body function.");
        let config = match &self.config {
            Some(c) => c.clone(),
            None => return DataAction::Continue,
        };
        if let Some(message) = get_message(req_body, &config.key_from.request_body) {
            if self.http_call_intent(&config, &message) {
                DataAction::StopIterationAndBuffer
            } else {
                DataAction::Continue
            }
        } else {
            DataAction::Continue
        }
    }
}