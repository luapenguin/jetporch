// Jetporch
// Copyright (C) 2023 - Michael DeHaan <michael@michaeldehaan.net> + contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// at your option) any later version.
// 
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
// 
// You should have received a copy of the GNU General Public License
// long with this program.  If not, see <http://www.gnu.org/licenses/>.

use std::sync::{Arc,RwLock};
use std::path::PathBuf;
use crate::tasks::request::TaskRequest;
use crate::tasks::response::TaskResponse;
use crate::inventory::hosts::Host;
use crate::playbooks::traversal::RunState;
use crate::playbooks::context::PlaybookContext;
use crate::tasks::cmd_library::{screen_path,screen_general_input_strict};
use crate::handle::response::Response;

#[derive(Eq,Hash,PartialEq,Clone,Copy,Debug)]
pub enum BlendTarget {
    NotTemplateModule,
    TemplateModule,
}

#[derive(Eq,Hash,PartialEq,Clone,Copy,Debug)]
pub enum Safety {
    Safe,
    Unsafe
}

pub struct Template {
    run_state: Arc<RunState>, 
    host: Arc<RwLock<Host>>, 
    response: Arc<Response>,
    syntax_only: bool
}

impl Template {

    pub fn new(run_state_handle: Arc<RunState>, host_handle: Arc<RwLock<Host>>, response:Arc<Response>) -> Self {
        let syntax_value = run_state_handle.visitor.read().unwrap().is_syntax_only();
        Self {
            run_state: run_state_handle,
            host: host_handle,
            response: response,
            syntax_only: syntax_value
        }
    }

    fn is_syntax(&self) -> bool {
        return self.syntax_only;
    }

    fn contains_vars(&self, input: &String) -> bool {
        if input.find("{{").is_some() {
            return true;
        }
        return false;
    }

    fn is_syntax_skip_eval(&self, template: &String) -> bool {
        if self.contains_vars(template) && self.is_syntax() {
            return true;
        }
        return false;
    }

    fn is_syntax_skip_eval_option(&self, template: &Option<String>) -> bool {
        if template.is_none() {
            return false;
        }
        let template2 = template.as_ref().unwrap();
        if self.contains_vars(&template2) && self.is_syntax() {
            return true;
        }
        return false;
    }

    pub fn get_context(&self) -> Arc<RwLock<PlaybookContext>> {
        return Arc::clone(&self.run_state.context);
    }

    fn unwrap_string_result(&self, request: &Arc<TaskRequest>, str_result: &Result<String,String>) -> Result<String, Arc<TaskResponse>> {
        return match str_result {
            Ok(x) => Ok(x.clone()),
            Err(y) => {
                return Err(self.response.is_failed(request, &y.clone()));
            }
        };
    }

    fn template_unsafe_internal(&self, request: &Arc<TaskRequest>, _field: &String, template: &String, blend_target: BlendTarget) -> Result<String,Arc<TaskResponse>> {
        if self.is_syntax_skip_eval(&template) {
            return Ok(String::from(""));
        }
        // note to module authors:
        // if you have a path, call template_path instead!  Do not call template_str as you will ignore path sanity checks.
        let result = self.run_state.context.read().unwrap().render_template(template, &self.host, blend_target);
        if result.is_ok() {
            let result_ok = result.as_ref().unwrap();
            if result_ok.eq("") {
                return Err(self.response.is_failed(request, &format!("evaluated to empty string")));
            }
        }
        let result2 = self.unwrap_string_result(request, &result)?;
        return Ok(result2);
    }
    
    pub fn string_for_template_module_use_only(&self, request: &Arc<TaskRequest>, field: &String, template: &String) -> Result<String,Arc<TaskResponse>> {
        return self.template_unsafe_internal(request, field, template, BlendTarget::TemplateModule);
    }

    pub fn string_unsafe(&self, request: &Arc<TaskRequest>, field: &String, template: &String) -> Result<String,Arc<TaskResponse>> {
        return self.template_unsafe_internal(request, field, template, BlendTarget::NotTemplateModule);
    }

    pub fn string(&self, request: &Arc<TaskRequest>, field: &String, template: &String) -> Result<String,Arc<TaskResponse>> {
        if self.is_syntax_skip_eval(&template) {
            return Ok(String::from(""));
        }
        let result = self.string_unsafe(request, field, template);
        return match result {
            Ok(x) => match screen_general_input_strict(&x) {
                Ok(y) => Ok(y),
                Err(z) => { return Err(self.response.is_failed(request, &format!("field {}, {}", field, z))) }
            },
            Err(y) => Err(y)
        };
    }

    pub fn string_no_spaces(&self, request: &Arc<TaskRequest>, field: &String, template: &String) -> Result<String,Arc<TaskResponse>> {
        let value = self.string(request, field, template)?;
        if self.has_spaces(&value) {
            return Err(self.response.is_failed(request, &format!("field ({}): spaces are not allowed", field)))
        }
        return Ok(value.clone());
    }

    pub fn string_option_no_spaces(&self, request: &Arc<TaskRequest>, field: &String, template: &Option<String>) -> Result<Option<String>,Arc<TaskResponse>> {
        let prelim = self.string_option(request, field, template)?;
        if prelim.is_some() {
            let value = prelim.as_ref().unwrap();
            if self.has_spaces(&value) {
                return Err(self.response.is_failed(request, &format!("field ({}): spaces are not allowed", field)))
            }
        }
        return Ok(prelim.clone());
    }


    pub fn no_template_string_option_trim(&self, input: &Option<String>) -> Option<String> {
        if input.is_some() {
            let value = input.as_ref().unwrap();
            return Some(value.trim().to_string());
        }
        return None;
    }

    pub fn path(&self, request: &Arc<TaskRequest>, field: &String, template: &String) -> Result<String,Arc<TaskResponse>> {
        if self.is_syntax_skip_eval(&template) {
            return Ok(String::from(""));
        }
        let result = self.run_state.context.read().unwrap().render_template(template, &self.host, BlendTarget::NotTemplateModule);
        let result2 = self.unwrap_string_result(request, &result)?;
        return match screen_path(&result2) {
            Ok(x) => Ok(x), Err(y) => { return Err(self.response.is_failed(request, &format!("{}, for field {}", y, field))) }
        }
    }

    pub fn string_option_unsafe(&self, request: &Arc<TaskRequest>,field: &String, template: &Option<String>) -> Result<Option<String>,Arc<TaskResponse>> {
        if self.is_syntax_skip_eval_option(template) {
            return Ok(Some(String::from("")));
        }
        if template.is_none() { return Ok(None); }
        let result = self.string(request, field, &template.as_ref().unwrap());
        return match result { 
            Ok(x) => Ok(Some(x)), 
            Err(y) => { Err(self.response.is_failed(request, &format!("field ({}) template error: {:?}", field, y))) } 
        };
    }

    pub fn string_option(&self, request: &Arc<TaskRequest>, field: &String, template: &Option<String>) -> Result<Option<String>,Arc<TaskResponse>> {
        if self.is_syntax_skip_eval_option(&template) {
            return Ok(Some(String::from("")));
        }
        let result = self.string_option_unsafe(request, field, template);
        return match result {
            Ok(x1) => match x1 {
                Some(x) => match screen_general_input_strict(&x) {
                    Ok(y) => Ok(Some(y)),
                    Err(z) => { return Err(self.response.is_failed(request, &format!("field {}, {}", field, z))) }
                },
                None => Ok(None)
            },
            Err(y) => Err(y)
        };
    }

    pub fn integer(&self, request: &Arc<TaskRequest>, field: &String, template: &String)-> Result<i64,Arc<TaskResponse>> {
        if self.is_syntax_skip_eval(&template) {
            return Ok(0);
        }
        let st = self.string(request, field, template)?;
        let num = st.parse::<i64>();
        return match num {
            Ok(num) => Ok(num), Err(_err) => Err(self.response.is_failed(request, &format!("field ({}) value is not an integer: {}", field, st)))
        }
    }

    pub fn integer_option(&self, request: &Arc<TaskRequest>, field: &String, template: &Option<String>) -> Result<Option<i64>,Arc<TaskResponse>> {
        if self.is_syntax_skip_eval_option(&template) {
            return Ok(Some(0));
        }
        if template.is_none() { return Ok(None); }
        let st = self.string(request, field, &template.as_ref().unwrap())?;
        let num = st.parse::<i64>();
        // FIXME: these can use map_err
        return match num {
            Ok(num) => Ok(Some(num)), Err(_err) => Err(self.response.is_failed(request, &format!("field ({}) value is not an integer: {}", field, st)))
        }
    }

    pub fn boolean(&self, request: &Arc<TaskRequest>, field: &String, template: &String) -> Result<bool,Arc<TaskResponse>> {
        if self.is_syntax_skip_eval(&template) {
            return Ok(false);
        }
        let st = self.string(request,field, template)?;
        let x = st.parse::<bool>();
        return match x {
            Ok(x) => Ok(x), Err(_err) => Err(self.response.is_failed(request, &format!("field ({}) value is not a boolean: {}", field, st)))
        }
    }

    pub fn boolean_option_default_true(&self, request: &Arc<TaskRequest>, field: &String, template: &Option<String>)-> Result<bool,Arc<TaskResponse>>{
        return self.internal_boolean_option(request, field, template, true);
    }

    pub fn boolean_option_default_false(&self, request: &Arc<TaskRequest>, field: &String, template: &Option<String>)-> Result<bool,Arc<TaskResponse>>{
        return self.internal_boolean_option(request, field, template, false);
    }
  
    fn internal_boolean_option(&self, request: &Arc<TaskRequest>, field: &String, template: &Option<String>, default: bool)-> Result<bool,Arc<TaskResponse>>{
        if self.is_syntax_skip_eval_option(&template) || template.is_none() {
            return Ok(default);
        }
        let st = self.string(request, field, &template.as_ref().unwrap())?;
        let x = st.parse::<bool>();
        return match x {
            Ok(x) => Ok(x),
            Err(_err) => Err(self.response.is_failed(request, &format!("field ({}) value is not a boolean: {}", field, st)))
        }
    }

    pub fn boolean_option_default_none(&self, request: &Arc<TaskRequest>, field: &String, template: &Option<String>)-> Result<Option<bool>,Arc<TaskResponse>>{
        if self.is_syntax_skip_eval_option(&template) || template.is_none() {
            return Ok(None);
        }
        let st = self.string(request, field, &template.as_ref().unwrap())?;
        let x = st.parse::<bool>();
        return match x {
            Ok(x) => Ok(Some(x)), 
            Err(_err) => Err(self.response.is_failed(request, &format!("field ({}) value is not a boolean: {}", field, st)))
        }
    }

    pub fn test_cond(&self, request: &Arc<TaskRequest>, expr: &String) -> Result<bool, Arc<TaskResponse>> {
        if self.is_syntax_skip_eval(&expr) {
            return Ok(true);
        }
        let result = self.get_context().read().unwrap().test_cond(expr, &self.host);
        return match result {
            Ok(x) => Ok(x), Err(y) => Err(self.response.is_failed(request, &y))
        }
    }

    #[inline]
    pub fn find_template_path(&self, request: &Arc<TaskRequest>, field: &String, str_path: &String) -> Result<PathBuf, Arc<TaskResponse>> {
        return self.find_sub_path(&String::from("templates"), request, field, str_path);
    }

    pub fn find_file_path(&self, request: &Arc<TaskRequest>, field: &String, str_path: &String) -> Result<PathBuf, Arc<TaskResponse>> {
        return self.find_sub_path(&String::from("files"), request, field, str_path);
    }

    fn find_sub_path(&self, prefix: &String, request: &Arc<TaskRequest>, field: &String, str_path: &String) -> Result<PathBuf, Arc<TaskResponse>> {
        if self.is_syntax_skip_eval(&str_path) {
            return Ok(PathBuf::new());
        }
        let prelim = match screen_path(&str_path) {
            Ok(x) => x, 
            Err(y) => { return Err(self.response.is_failed(request, &format!("{}, for field: {}", y, field))) }
        };
        let mut path = PathBuf::new();
        path.push(prelim);
        if path.is_absolute() {
            if path.is_file() {
                return Ok(path);
            } else {
                return Err(self.response.is_failed(request, &format!("field ({}): no such file: {}", field, str_path)));
            }
        } else {
            let mut path2 = PathBuf::new();
            path2.push(prefix);
            path2.push(str_path);
            if path2.is_file() {
                return Ok(path2);
            } else {
                return Err(self.response.is_failed(request, &format!("field field ({}): no such file: {}", field, str_path)));
            }
        }
    }

    pub fn has_spaces(&self, input: &String) -> bool {
        let found = input.find(" ");
        return found.is_some();
    }

}