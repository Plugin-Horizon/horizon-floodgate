//! Bedrock forms as Floodgate sends them over `floodgate:form`: GeyserMC's Cumulus 1.1 form model (MIT, see
//! LICENSE): the JSON its codecs write (`ModalFormCodec`, `SimpleFormCodec`, `CustomFormCodec`, the component
//! classes' fields) and how they read the client's answer back. Optional buttons and components (Cumulus'
//! `optionalButton`/`optionalComponent` with `false`) are `None`: left out of the JSON, and the answer is mapped
//! back over them.

use serde_json::{json, Map, Value as Json};

/// Cumulus' `FormType`; the ordinal is the first byte of a form's channel data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormType {
    Simple = 0,
    Modal = 1,
    Custom = 2,
}

/// `FormImage`: a resource-pack path or a URL.
#[derive(Clone, Debug, PartialEq)]
pub enum Image {
    Path(String),
    Url(String),
}

impl Image {
    fn json(&self) -> Json {
        match self {
            Image::Path(p) => json!({ "type": "path", "data": p }),
            Image::Url(u) => json!({ "type": "url", "data": u }),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Button {
    pub text: String,
    pub image: Option<Image>,
}

impl Button {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.into(),
            image: None,
        }
    }
}

/// The components of a custom form (`ComponentType`).
#[derive(Clone, Debug, PartialEq)]
pub enum Component {
    Dropdown {
        text: String,
        options: Vec<String>,
        default: usize,
    },
    Input {
        text: String,
        placeholder: String,
        default: String,
    },
    Label {
        text: String,
    },
    /// `default: None` takes Cumulus' generated middle value.
    Slider {
        text: String,
        min: f32,
        max: f32,
        step: f32,
        default: Option<f32>,
    },
    StepSlider {
        text: String,
        steps: Vec<String>,
        default: usize,
    },
    Toggle {
        text: String,
        default: bool,
    },
}

/// `SliderComponentImpl.generateDefaultValue`.
pub fn slider_default(min: f32, max: f32, step: f32) -> f32 {
    let middle = min + (max - min) / 2.0;
    if ((middle - min) / step) % 1.0 == 0.0 {
        return middle;
    }
    if min + step * 50.0 < max {
        return min;
    }
    let mut previous = min;
    while previous < max {
        let next = previous + step;
        if next > middle {
            return previous;
        }
        previous = next;
    }
    previous
}

/// A default index past the end becomes 0, as Cumulus' constructors do.
fn index_or_zero(default: usize, len: usize) -> usize {
    if default >= len {
        0
    } else {
        default
    }
}

impl Component {
    fn json(&self) -> Json {
        match self {
            Component::Dropdown {
                text,
                options,
                default,
            } => json!({ "type": "dropdown", "text": text, "options": options,
                "default": index_or_zero(*default, options.len()) }),
            Component::Input {
                text,
                placeholder,
                default,
            } => {
                json!({ "type": "input", "text": text, "placeholder": placeholder, "default": default })
            }
            Component::Label { text } => json!({ "type": "label", "text": text }),
            Component::Slider {
                text,
                min,
                max,
                step,
                default,
            } => json!({ "type": "slider", "text": text, "min": min, "max": max, "step": step,
                "default": default.unwrap_or_else(|| slider_default(*min, *max, *step)) }),
            Component::StepSlider {
                text,
                steps,
                default,
            } => json!({ "type": "step_slider", "text": text, "steps": steps,
                "default": index_or_zero(*default, steps.len()) }),
            Component::Toggle { text, default } => {
                json!({ "type": "toggle", "text": text, "default": default })
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Form {
    Modal {
        title: String,
        content: String,
        button1: String,
        button2: String,
    },
    Simple {
        title: String,
        content: String,
        buttons: Vec<Option<Button>>,
    },
    Custom {
        title: String,
        icon: Option<Image>,
        content: Vec<Option<Component>>,
    },
}

impl Form {
    pub fn form_type(&self) -> FormType {
        match self {
            Form::Modal { .. } => FormType::Modal,
            Form::Simple { .. } => FormType::Simple,
            Form::Custom { .. } => FormType::Custom,
        }
    }

    /// The form as the Bedrock client reads it (`FormCodec.jsonData`). Keys whose value is null are left out,
    /// as Gson does by default.
    pub fn to_json(&self) -> String {
        let mut o = Map::new();
        match self {
            Form::Modal {
                title,
                content,
                button1,
                button2,
            } => {
                o.insert("title".into(), json!(title));
                o.insert("content".into(), json!(content));
                o.insert("button1".into(), json!(button1));
                o.insert("button2".into(), json!(button2));
                o.insert("type".into(), json!("modal"));
            }
            Form::Simple {
                title,
                content,
                buttons,
            } => {
                o.insert("title".into(), json!(title));
                o.insert("content".into(), json!(content));
                let buttons: Vec<Json> = buttons
                    .iter()
                    .flatten()
                    .map(|b| {
                        let mut m = Map::new();
                        m.insert("text".into(), json!(b.text));
                        if let Some(i) = &b.image {
                            m.insert("image".into(), i.json());
                        }
                        Json::Object(m)
                    })
                    .collect();
                o.insert("buttons".into(), Json::Array(buttons));
                o.insert("type".into(), json!("form"));
            }
            Form::Custom {
                title,
                icon,
                content,
            } => {
                o.insert("title".into(), json!(title));
                if let Some(i) = icon {
                    o.insert("icon".into(), i.json());
                }
                let content: Vec<Json> = content.iter().flatten().map(Component::json).collect();
                o.insert("content".into(), Json::Array(content));
                o.insert("type".into(), json!("custom_form"));
            }
        }
        Json::Object(o).to_string()
    }
}

/// One answer of a custom form, in the order of `content` (optional components that were left out included).
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// A left-out optional component (`AbsentComponent`).
    Absent,
    /// A label answers null.
    Label,
    Text(String),
    Float(f32),
    Int(i32),
    Bool(bool),
}

/// `FormResponseResult`: closed, invalid (the index of the component that failed, or -1), or valid.
#[derive(Clone, Debug, PartialEq)]
pub enum Response {
    Closed,
    Invalid {
        index: i32,
        reason: String,
    },
    /// `ModalFormResponse`: 0 for `button1` (the client said `true`), 1 for `button2`.
    Modal {
        clicked: usize,
        text: String,
    },
    /// `SimpleFormResponse`: the index in `buttons`, counting left-out ones.
    Simple {
        clicked: usize,
        text: String,
    },
    Custom {
        values: Vec<Value>,
    },
}

fn invalid(index: i32, reason: &str) -> Response {
    Response::Invalid {
        index,
        reason: reason.into(),
    }
}

/// `FormCodecImpl.deserializeFormResponse`: the client's answer to `form`.
pub fn parse_response(form: &Form, data: &str) -> Response {
    if data.is_empty() || data.trim() == "null" {
        return Response::Closed;
    }
    match form {
        Form::Modal {
            button1, button2, ..
        } => match data.trim() {
            "true" => Response::Modal {
                clicked: 0,
                text: button1.clone(),
            },
            "false" => Response::Modal {
                clicked: 1,
                text: button2.clone(),
            },
            _ => invalid(-1, "Response wasn't a boolean"),
        },
        Form::Simple { buttons, .. } => {
            let Ok(mut id) = data.trim().parse::<i32>() else {
                return invalid(
                    -1,
                    "Received invalid integer representing the clicked button",
                );
            };
            if id < 0 {
                return invalid(-1, "Received a clicked button id that's smaller than 0");
            }
            for (i, b) in buttons.iter().enumerate() {
                if let Some(b) = b {
                    if id == 0 {
                        return Response::Simple {
                            clicked: i,
                            text: b.text.clone(),
                        };
                    }
                    id -= 1;
                }
            }
            invalid(
                -1,
                "Receiver a button id larger than the amount of buttons in the form",
            )
        }
        Form::Custom { content, .. } => parse_custom(content, data),
    }
}

fn parse_custom(content: &[Option<Component>], data: &str) -> Response {
    let Ok(Json::Array(answers)) = serde_json::from_str::<Json>(data) else {
        return invalid(-1, "Response wasn't a JSON array");
    };
    let mut values = Vec::with_capacity(content.len());
    let mut next = answers.iter();
    for (i, component) in content.iter().enumerate() {
        let Some(component) = component else {
            values.push(Value::Absent);
            continue;
        };
        let Some(answer) = next.next() else {
            return invalid(-1, "Response doesn't contain enough components");
        };
        match check(component, answer) {
            Ok(v) => values.push(v),
            Err(reason) => return invalid(i as i32, &reason),
        }
    }
    if next.next().is_some() {
        return invalid(-1, "Response contains too many elements");
    }
    Response::Custom { values }
}

/// `CustomFormCodec.validateComponent`. Like Gson's `getAsInt`, a number with a fraction is cut.
fn check(component: &Component, answer: &Json) -> Result<Value, String> {
    let name = match component {
        Component::Dropdown { .. } => "dropdown",
        Component::Input { .. } => "input",
        Component::Label { .. } => "label",
        Component::Slider { .. } => "slider",
        Component::StepSlider { .. } => "step slider",
        Component::Toggle { .. } => "toggle",
    };
    if let Component::Label { .. } = component {
        return if answer.is_null() {
            Ok(Value::Label)
        } else {
            Err("Return value of label should be null".into())
        };
    }
    if answer.is_array() || answer.is_object() || answer.is_null() {
        return Err(format!("Return value of {name} should be a json primitive"));
    }
    let int = || answer.as_f64().map(|f| f as i32);
    match component {
        Component::Input { .. } => answer
            .as_str()
            .map(|s| Value::Text(s.into()))
            .ok_or_else(|| "Return value of input should be a string".into()),
        Component::Slider { .. } => answer
            .as_f64()
            .map(|f| Value::Float(f as f32))
            .ok_or_else(|| "Return value of slider should be a float".into()),
        Component::StepSlider { .. } => int()
            .map(Value::Int)
            .ok_or_else(|| "Return value of step slider should be an integer".into()),
        Component::Toggle { .. } => answer
            .as_bool()
            .map(Value::Bool)
            .ok_or_else(|| "Return value of toggle should be a boolean".into()),
        Component::Dropdown { .. } => int()
            .map(Value::Int)
            .ok_or_else(|| "Return value of dropdown should be an integer".into()),
        Component::Label { .. } => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(s: &str) -> Json {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn modal_json_and_answers() {
        let f = Form::Modal {
            title: "T".into(),
            content: "C".into(),
            button1: "Yes".into(),
            button2: "No".into(),
        };
        assert_eq!(f.form_type() as u8, 1);
        assert_eq!(
            parsed(&f.to_json()),
            json!({"type":"modal","title":"T","content":"C","button1":"Yes","button2":"No"})
        );
        assert_eq!(
            parse_response(&f, "true\n"),
            Response::Modal {
                clicked: 0,
                text: "Yes".into()
            }
        );
        assert_eq!(
            parse_response(&f, "false"),
            Response::Modal {
                clicked: 1,
                text: "No".into()
            }
        );
        assert_eq!(parse_response(&f, ""), Response::Closed);
        assert_eq!(parse_response(&f, " null "), Response::Closed);
        assert!(matches!(
            parse_response(&f, "1"),
            Response::Invalid { index: -1, .. }
        ));
    }

    #[test]
    fn simple_form_skips_left_out_buttons() {
        let f = Form::Simple {
            title: "T".into(),
            content: "".into(),
            buttons: vec![
                Some(Button::new("a")),
                None,
                Some(Button {
                    text: "c".into(),
                    image: Some(Image::Url("https://example.org/i.png".into())),
                }),
            ],
        };
        assert_eq!(f.form_type() as u8, 0);
        assert_eq!(
            parsed(&f.to_json()),
            json!({"type":"form","title":"T","content":"","buttons":[{"text":"a"},
                {"text":"c","image":{"type":"url","data":"https://example.org/i.png"}}]})
        );
        assert_eq!(
            parse_response(&f, "1"),
            Response::Simple {
                clicked: 2,
                text: "c".into()
            }
        );
        assert!(matches!(parse_response(&f, "2"), Response::Invalid { .. }));
        assert!(matches!(parse_response(&f, "-1"), Response::Invalid { .. }));
    }

    #[test]
    fn custom_form_json_and_answers() {
        let f = Form::Custom {
            title: "Settings".into(),
            icon: Some(Image::Path("textures/ui/gear".into())),
            content: vec![
                Some(Component::Label { text: "hi".into() }),
                None,
                Some(Component::Input {
                    text: "Name".into(),
                    placeholder: "name".into(),
                    default: "".into(),
                }),
                Some(Component::Slider {
                    text: "Vol".into(),
                    min: 0.0,
                    max: 10.0,
                    step: 1.0,
                    default: None,
                }),
                Some(Component::Dropdown {
                    text: "D".into(),
                    options: vec!["x".into(), "y".into()],
                    default: 5,
                }),
                Some(Component::Toggle {
                    text: "On".into(),
                    default: true,
                }),
                Some(Component::StepSlider {
                    text: "S".into(),
                    steps: vec!["lo".into(), "hi".into()],
                    default: 1,
                }),
            ],
        };
        assert_eq!(f.form_type() as u8, 2);
        let j = parsed(&f.to_json());
        assert_eq!(j["type"], "custom_form");
        assert_eq!(j["icon"], json!({"type":"path","data":"textures/ui/gear"}));
        assert_eq!(j["content"].as_array().unwrap().len(), 6);
        assert_eq!(j["content"][2]["default"], json!(5.0));
        assert_eq!(j["content"][3]["default"], json!(0));
        assert_eq!(
            parse_response(&f, r#"[null,"Steve",3,1,false,0]"#),
            Response::Custom {
                values: vec![
                    Value::Label,
                    Value::Absent,
                    Value::Text("Steve".into()),
                    Value::Float(3.0),
                    Value::Int(1),
                    Value::Bool(false),
                    Value::Int(0)
                ]
            }
        );
        assert_eq!(
            parse_response(&f, r#"[null,"Steve",3,1,"no",0]"#),
            Response::Invalid {
                index: 5,
                reason: "Return value of toggle should be a boolean".into()
            }
        );
        assert!(matches!(
            parse_response(&f, "[null]"),
            Response::Invalid { index: -1, .. }
        ));
        assert!(matches!(
            parse_response(&f, r#"[null,"a",1,1,true,0,9]"#),
            Response::Invalid { index: -1, .. }
        ));
    }

    #[test]
    fn slider_defaults_like_cumulus() {
        assert_eq!(slider_default(0.0, 10.0, 1.0), 5.0);
        assert_eq!(slider_default(0.0, 10.0, 3.0), 3.0);
        assert_eq!(slider_default(0.0, 1000.0, 3.0), 0.0);
    }
}
