// Copyright (c) 2022 Alibaba Group Holding Ltd.
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

package tests

import (
	"testing"

	"github.com/alibaba/higress/test/e2e/conformance/utils/http"
	"github.com/alibaba/higress/test/e2e/conformance/utils/suite"
)

func init() {
	Register(RustWasmPluginsAiDataMasking)
}

func gen_assertion(host string, req_body []byte, res_body []byte) http.Assertion {
	return http.Assertion{
		Meta: http.AssertionMeta{
			TargetBackend:   "infra-backend-v1",
			TargetNamespace: "higress-conformance-infra",
		},
		Request: http.AssertionRequest{
			ActualRequest: http.Request{
				Host:             host,
				Path:             "/",
				Method:           "POST",
				ContentType:      http.ContentTypeApplicationJson,
				Body:             req_body,
				UnfollowRedirect: true,
			},
		},
		Response: http.AssertionResponse{
			ExpectedResponse: http.Response{
				Body: res_body,
			},
		},
	}
}

var RustWasmPluginsAiDataMasking = suite.ConformanceTest{
	ShortName:   "RustWasmPluginsAiDataMasking",
	Description: "The Ingress in the higress-conformance-infra namespace test the rust ai-data-masking wasmplugins.",
	Manifests:   []string{"tests/rust-wasm-ai-data-masking.yaml"},
	Features:    []suite.SupportedFeature{suite.WASMRustConformanceFeature},
	Test: func(t *testing.T, suite *suite.ConformanceTestSuite) {
		var testcases []http.Assertion
		//openai
		testcases = append(testcases, gen_assertion(
			"replace.openai.com",
			[]byte("{\"messages\":[{\"role\":\"user\",\"content\":\"127.0.0.1 admin@gmail.com sk-12345\"}]}"),
			[]byte("{\"choices\":[{\"index\":0,\"message\":{\"role\":\"assistant\",\"content\":\"127.0.0.1 sk12345 admin@gmail.com\"}}],\"usage\":{}}"),
		))
		testcases = append(testcases, gen_assertion(
			"ok.openai.com",
			[]byte("{\"messages\":[{\"role\":\"user\",\"content\":\"fuck\"}]}"),
			[]byte("{\"choices\":[{\"index\":0,\"message\":{\"role\":\"assistant\",\"content\":\"提问或回答中包含敏感词，已被屏蔽\"}}],\"usage\":{}}"),
		))
		testcases = append(testcases, gen_assertion(
			"ok.openai.com",
			[]byte("{\"messages\":[{\"role\":\"user\",\"content\":\"costom_word1\"}]}"),
			[]byte("{\"choices\":[{\"index\":0,\"message\":{\"role\":\"assistant\",\"content\":\"提问或回答中包含敏感词，已被屏蔽\"}}],\"usage\":{}}"),
		))
		testcases = append(testcases, gen_assertion(
			"ok.openai.com",
			[]byte("{\"messages\":[{\"role\":\"user\",\"content\":\"costom_word\"}]}"),
			[]byte("{\"choices\":[{\"index\":0,\"message\":{\"role\":\"assistant\",\"content\":\"ok\"}}],\"usage\":{}}"),
		))

		testcases = append(testcases, gen_assertion(
			"system_deny.openai.com",
			[]byte("{\"messages\":[{\"role\":\"user\",\"content\":\"test\"}]}"),
			[]byte(""),
		))
		testcases = append(testcases, gen_assertion(
			"costom_word1.openai.com",
			[]byte("{\"messages\":[{\"role\":\"user\",\"content\":\"test\"}]}"),
			[]byte(""),
		))
		testcases = append(testcases, gen_assertion(
			"costom_word.openai.com",
			[]byte("{\"messages\":[{\"role\":\"user\",\"content\":\"test\"}]}"),
			[]byte(""),
		))

		//jsonpath
		testcases = append(testcases, gen_assertion(
			"ok.raw.com",
			[]byte("{\"test\":[{\"test\":\"costom\\\"word\"}]}"),
			[]byte("{\"errmsg\":\"提问或回答中包含敏感词，已被屏蔽\"}"),
		))
		testcases = append(testcases, gen_assertion(
			"ok.raw.com",
			[]byte("{\"test1\":[{\"test1\":\"costom\\\"word\"}]}"),
			[]byte("ok"),
		))

		//raw
		testcases = append(testcases, gen_assertion(
			"replace.raw.com",
			[]byte("127.0.0.1 admin@gmail.com sk-12345"),
			[]byte("127.0.0.1 sk12345 admin@gmail.com"),
		))

		testcases = append(testcases, gen_assertion(
			"ok.raw.com",
			[]byte("fuck"),
			[]byte("{\"errmsg\":\"提问或回答中包含敏感词，已被屏蔽\"}"),
		))
		testcases = append(testcases, gen_assertion(
			"ok.raw.com",
			[]byte("costom_word1"),
			[]byte("{\"errmsg\":\"提问或回答中包含敏感词，已被屏蔽\"}"),
		))
		testcases = append(testcases, gen_assertion(
			"ok.raw.com",
			[]byte("costom_word"),
			[]byte("ok"),
		))

		testcases = append(testcases, gen_assertion(
			"system_deny.openai.com",
			[]byte("test"),
			[]byte(""),
		))
		testcases = append(testcases, gen_assertion(
			"costom_word1.openai.com",
			[]byte("test"),
			[]byte(""),
		))
		testcases = append(testcases, gen_assertion(
			"costom_word.openai.com",
			[]byte("test"),
			[]byte("costom_word"),
		))

		t.Run("WasmPlugins ai-data-masking", func(t *testing.T) {
			for _, testcase := range testcases {
				http.MakeRequestAndExpectEventuallyConsistentResponse(t, suite.RoundTripper, suite.TimeoutConfig, suite.GatewayAddress, testcase)
			}
		})
	},
}