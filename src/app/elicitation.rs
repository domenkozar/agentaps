use super::*;
use serde_json::Map;

pub(super) struct Elicitation {
    pub(super) request_id: Value,
    pub(super) message: String,
    pub(super) fields: Vec<ElicitationField>,
    pub(super) error: Option<String>,
}

pub(super) struct ElicitationField {
    pub(super) key: String,
    pub(super) title: String,
    pub(super) description: Option<String>,
    pub(super) required: bool,
    pub(super) schema: Value,
    pub(super) kind: ElicitationFieldKind,
}

pub(super) enum ElicitationFieldKind {
    Input(Entity<InputState>),
    Select {
        options: Vec<(String, String)>,
        selected: Option<usize>,
    },
    MultiSelect {
        options: Vec<(String, String)>,
        selected: HashSet<usize>,
    },
    Boolean(Option<bool>),
}

impl Elicitation {
    pub(super) fn new(
        request_id: Value,
        params: &Value,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Result<Self, String> {
        if params["mode"].as_str() != Some("form") {
            return Err("Only form elicitation is supported".into());
        }
        let schema = &params["requestedSchema"];
        let properties = schema["properties"]
            .as_object()
            .ok_or("Form schema has no properties")?;
        let required = schema["required"].as_array();
        let mut fields = Vec::with_capacity(properties.len());
        for (key, property) in properties {
            let title = property["title"].as_str().unwrap_or(key).to_owned();
            let kind = match property["type"].as_str() {
                Some("string") => {
                    if let Some(options) = property["oneOf"].as_array() {
                        ElicitationFieldKind::Select {
                            options: options
                                .iter()
                                .filter_map(|item| {
                                    Some((
                                        item["const"].as_str()?.to_owned(),
                                        item["title"].as_str()?.to_owned(),
                                    ))
                                })
                                .collect(),
                            selected: options
                                .iter()
                                .position(|item| item["const"] == property["default"]),
                        }
                    } else if let Some(options) = property["enum"].as_array() {
                        ElicitationFieldKind::Select {
                            options: options
                                .iter()
                                .filter_map(Value::as_str)
                                .map(|value| (value.to_owned(), value.to_owned()))
                                .collect(),
                            selected: options.iter().position(|item| item == &property["default"]),
                        }
                    } else {
                        let default = property["default"].as_str().unwrap_or("").to_owned();
                        let input = cx.new(|cx| {
                            InputState::new(window, cx)
                                .placeholder(title.clone())
                                .default_value(default)
                        });
                        ElicitationFieldKind::Input(input)
                    }
                }
                Some("number" | "integer") => {
                    let default = property["default"]
                        .as_number()
                        .map(ToString::to_string)
                        .unwrap_or_default();
                    let input = cx.new(|cx| {
                        InputState::new(window, cx)
                            .placeholder(title.clone())
                            .default_value(default)
                    });
                    ElicitationFieldKind::Input(input)
                }
                Some("boolean") => ElicitationFieldKind::Boolean(property["default"].as_bool()),
                Some("array") => {
                    let options = property["items"]["anyOf"]
                        .as_array()
                        .or_else(|| property["items"]["enum"].as_array())
                        .ok_or_else(|| format!("Unsupported choices for {title}"))?;
                    ElicitationFieldKind::MultiSelect {
                        options: options
                            .iter()
                            .filter_map(|item| {
                                if let Some(value) = item.as_str() {
                                    Some((value.to_owned(), value.to_owned()))
                                } else {
                                    Some((
                                        item["const"].as_str()?.to_owned(),
                                        item["title"].as_str()?.to_owned(),
                                    ))
                                }
                            })
                            .collect(),
                        selected: options
                            .iter()
                            .enumerate()
                            .filter_map(|(index, item)| {
                                let value = item.as_str().or_else(|| item["const"].as_str())?;
                                property["default"]
                                    .as_array()?
                                    .iter()
                                    .any(|default| default == value)
                                    .then_some(index)
                            })
                            .collect(),
                    }
                }
                _ => return Err(format!("Unsupported field type for {title}")),
            };
            if matches!(&kind, ElicitationFieldKind::Select { options, .. } | ElicitationFieldKind::MultiSelect { options, .. } if options.is_empty())
            {
                return Err(format!("No choices were provided for {title}"));
            }
            fields.push(ElicitationField {
                key: key.clone(),
                title: property["title"].as_str().unwrap_or(key).to_owned(),
                description: property["description"].as_str().map(str::to_owned),
                required: required.is_some_and(|names| names.iter().any(|name| name == key)),
                schema: property.clone(),
                kind,
            });
        }
        Ok(Self {
            request_id,
            message: params["message"]
                .as_str()
                .unwrap_or("Agent asks a question")
                .into(),
            fields,
            error: None,
        })
    }

    pub(super) fn content(&self, cx: &Context<Workspace>) -> Result<Value, String> {
        let mut values = Map::new();
        for field in &self.fields {
            let value = match &field.kind {
                ElicitationFieldKind::Input(input) => {
                    let text = input.read(cx).value().to_string();
                    if text.is_empty() && !field.required {
                        continue;
                    }
                    match field.schema["type"].as_str() {
                        Some("integer") => json!(
                            text.parse::<i64>()
                                .map_err(|_| format!("{} must be an integer", field.title))?
                        ),
                        Some("number") => {
                            let number = text
                                .parse::<f64>()
                                .map_err(|_| format!("{} must be a number", field.title))?;
                            if !number.is_finite() {
                                return Err(format!("{} must be a finite number", field.title));
                            }
                            json!(number)
                        }
                        _ => json!(text),
                    }
                }
                ElicitationFieldKind::Select { options, selected } => match selected {
                    Some(index) => json!(options[*index].0),
                    None if field.required => return Err(format!("Choose {}", field.title)),
                    None => continue,
                },
                ElicitationFieldKind::MultiSelect { options, selected } => {
                    if selected.is_empty() && !field.required {
                        continue;
                    }
                    json!(
                        options
                            .iter()
                            .enumerate()
                            .filter(|(index, _)| selected.contains(index))
                            .map(|(_, option)| &option.0)
                            .collect::<Vec<_>>()
                    )
                }
                ElicitationFieldKind::Boolean(value) => match value {
                    Some(value) => json!(value),
                    None if field.required => return Err(format!("Choose {}", field.title)),
                    None => continue,
                },
            };
            validate_value(field, &value)?;
            values.insert(field.key.clone(), value);
        }
        Ok(Value::Object(values))
    }
}

fn validate_value(field: &ElicitationField, value: &Value) -> Result<(), String> {
    let schema = &field.schema;
    if let Some(text) = value.as_str() {
        let len = text.chars().count() as u64;
        if field.required && text.is_empty()
            || schema["minLength"].as_u64().is_some_and(|min| len < min)
            || schema["maxLength"].as_u64().is_some_and(|max| len > max)
        {
            return Err(format!("{} has an invalid length", field.title));
        }
        if let Some(pattern) = schema["pattern"].as_str() {
            let regex = regex::RegexBuilder::new(pattern)
                .size_limit(1_000_000)
                .build()
                .map_err(|_| format!("{} has an invalid pattern", field.title))?;
            if !regex.is_match(text) {
                return Err(format!(
                    "{} does not match the required pattern",
                    field.title
                ));
            }
        }
    }
    if let Some(number) = value.as_i64()
        && (schema["minimum"].as_i64().is_some_and(|min| number < min)
            || schema["maximum"].as_i64().is_some_and(|max| number > max))
    {
        return Err(format!("{} is outside the allowed range", field.title));
    }
    if let Some(number) = value.as_f64()
        && schema["type"].as_str() == Some("number")
        && (schema["minimum"].as_f64().is_some_and(|min| number < min)
            || schema["maximum"].as_f64().is_some_and(|max| number > max))
    {
        return Err(format!("{} is outside the allowed range", field.title));
    }
    if let Some(items) = value.as_array()
        && (schema["minItems"]
            .as_u64()
            .is_some_and(|min| items.len() < min as usize)
            || schema["maxItems"]
                .as_u64()
                .is_some_and(|max| items.len() > max as usize))
    {
        return Err(format!("{} has an invalid number of choices", field.title));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(schema: Value) -> ElicitationField {
        ElicitationField {
            key: "answer".into(),
            title: "Answer".into(),
            description: None,
            required: true,
            schema,
            kind: ElicitationFieldKind::Boolean(None),
        }
    }

    #[test]
    fn validates_agent_constraints_before_responding() {
        let text = field(json!({"type":"string","minLength":2,"maxLength":4,"pattern":"^[a-z]+$"}));
        assert!(validate_value(&text, &json!("a")).is_err());
        assert!(validate_value(&text, &json!("123")).is_err());
        assert!(validate_value(&text, &json!("okay")).is_ok());

        let integer = field(json!({"type":"integer","minimum":10,"maximum":20}));
        assert!(validate_value(&integer, &json!(9)).is_err());
        assert!(validate_value(&integer, &json!(20)).is_ok());

        let choices = field(json!({"type":"array","minItems":1,"maxItems":2}));
        assert!(validate_value(&choices, &json!([])).is_err());
        assert!(validate_value(&choices, &json!(["a", "b"])).is_ok());
    }
}
