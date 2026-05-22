use rig_core::completion::ToolDefinition;
use rig_core::tool::{ToolDyn, ToolError};
use rig_core::wasm_compat::WasmBoxedFuture;
use std::sync::Arc;

#[derive(Clone)]
pub struct DynTool(pub Arc<dyn ToolDyn + 'static>);

impl ToolDyn for DynTool {
    fn name(&self) -> String {
        self.0.name()
    }

    fn definition<'a>(&'a self, prompt: String) -> WasmBoxedFuture<'a, ToolDefinition> {
        Box::pin(async move { self.0.definition(prompt).await })
    }

    fn call<'a>(&'a self, args: String) -> WasmBoxedFuture<'a, Result<String, ToolError>> {
        Box::pin(async move { self.0.call(args).await })
    }
}
