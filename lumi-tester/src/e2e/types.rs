use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};

/// Capabilities that the Phase 1 Home Controller boundary can require.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum E2eCapability {
    Health,
    DeviceDiscovery,
    DeviceState,
    DeviceControl,
    RealtimeCellReadback,
}

impl E2eCapability {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "health" => Some(Self::Health),
            "device_discovery" => Some(Self::DeviceDiscovery),
            "device_state" => Some(Self::DeviceState),
            "device_control" => Some(Self::DeviceControl),
            "realtime_cell_readback" => Some(Self::RealtimeCellReadback),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Health => "health",
            Self::DeviceDiscovery => "device_discovery",
            Self::DeviceState => "device_state",
            Self::DeviceControl => "device_control",
            Self::RealtimeCellReadback => "realtime_cell_readback",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct E2eEntityParams {
    pub entity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct E2eIndexedValue {
    pub idx: u64,
    pub value: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct E2eStateParams {
    pub entity: String,
    pub state: Vec<E2eIndexedValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct E2eSubscribeParams {
    pub channels: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct E2eExpectParams {
    pub entity: String,
    pub event: Value,
}

/// Semantic operations stay separate from App driver commands.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum E2eCommand {
    HcHealth,
    HcGetEntities,
    HcGetState(E2eEntityParams),
    HcControl(E2eStateParams),
    HcWaitState(E2eStateParams),
    WsSubscribe(E2eSubscribeParams),
    WsExpect(E2eExpectParams),
    WsSnapshot(E2eEntityParams),
}

impl E2eCommand {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::HcHealth => "hcHealth",
            Self::HcGetEntities => "hcGetEntities",
            Self::HcGetState(_) => "hcGetState",
            Self::HcControl(_) => "hcControl",
            Self::HcWaitState(_) => "hcWaitState",
            Self::WsSubscribe(_) => "wsSubscribe",
            Self::WsExpect(_) => "wsExpect",
            Self::WsSnapshot(_) => "wsSnapshot",
        }
    }

    pub fn required_capabilities(&self) -> Vec<E2eCapability> {
        match self {
            Self::HcHealth => vec![E2eCapability::Health],
            Self::HcGetEntities => vec![E2eCapability::DeviceDiscovery],
            Self::HcGetState(_) | Self::HcWaitState(_) => {
                vec![E2eCapability::DeviceState]
            }
            // This is the advertised capability union for the command. The
            // two readback entries are alternatives, not an all-of gate;
            // validate_flow enforces the explicit device_state OR
            // realtime_cell_readback rule for HcControl.
            Self::HcControl(_) => vec![
                E2eCapability::DeviceControl,
                E2eCapability::DeviceState,
                E2eCapability::RealtimeCellReadback,
            ],
            Self::WsSubscribe(_) | Self::WsExpect(_) | Self::WsSnapshot(_) => {
                vec![E2eCapability::RealtimeCellReadback]
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct E2eCommandRequirement {
    pub index: usize,
    pub name: String,
    /// Advertised capability union. HcControl's readback entries are an
    /// any-of alternative enforced by profile preflight, not a conjunction.
    pub capabilities: Vec<E2eCapability>,
}

impl E2eCommandRequirement {
    pub fn for_command(index: usize, command: &E2eCommand) -> Self {
        Self {
            index,
            name: command.display_name().to_string(),
            capabilities: command.required_capabilities(),
        }
    }
}

/// Return an actionable message for operations that are intentionally deferred
/// by the single-device Phase 1 contract.
pub fn deferred_command_message(name: &str) -> Option<String> {
    let deferred = matches!(
        name,
        "hcConfig"
            | "hcExecute"
            | "groupControl"
            | "sceneControl"
            | "scheduleControl"
            | "ruleControl"
            | "cloudRequest"
            | "mqttPublish"
            | "sshCommand"
            | "serialCommand"
            | "bridgeScan"
            | "hardwareJig"
    );

    deferred.then(|| {
        format!(
            "E2E command '{name}' is deferred outside the single-device system_journey scope"
        )
    })
}

pub fn parse_e2e_command(name: &str, params: &Value) -> anyhow::Result<Option<E2eCommand>> {
    let command = match name {
        "hcHealth" => {
            ensure_no_params(name, params)?;
            Some(E2eCommand::HcHealth)
        }
        "hcGetEntities" => {
            ensure_no_params(name, params)?;
            Some(E2eCommand::HcGetEntities)
        }
        "hcGetState" => Some(E2eCommand::HcGetState(parse_entity_params(name, params)?)),
        "hcControl" => Some(E2eCommand::HcControl(parse_state_params(name, params)?)),
        "hcWaitState" => Some(E2eCommand::HcWaitState(parse_state_params(name, params)?)),
        "wsSubscribe" => Some(E2eCommand::WsSubscribe(parse_subscribe_params(name, params)?)),
        "wsExpect" => Some(E2eCommand::WsExpect(parse_expect_params(name, params)?)),
        "wsSnapshot" => Some(E2eCommand::WsSnapshot(parse_entity_params(name, params)?)),
        _ => None,
    };

    Ok(command)
}

fn parse_entity_params(name: &str, params: &Value) -> anyhow::Result<E2eEntityParams> {
    let map = params_mapping(name, params)?;
    Ok(E2eEntityParams {
        entity: required_string(map, "entity", name)?,
    })
}

fn parse_state_params(name: &str, params: &Value) -> anyhow::Result<E2eStateParams> {
    let map = params_mapping(name, params)?;
    if map.contains_key(Value::String("capability".to_string())) {
        anyhow::bail!(
            "E2E command '{name}' uses deferred capability alias; use explicit state with idx and value"
        );
    }

    let entity = required_string(map, "entity", name)?;
    let state = map
        .get(Value::String("state".to_string()))
        .ok_or_else(|| anyhow::anyhow!("E2E command '{name}' requires 'state'"))?;
    let values = state
        .as_sequence()
        .ok_or_else(|| anyhow::anyhow!("E2E command '{name}' field 'state' must be a list"))?;

    if values.is_empty() {
        anyhow::bail!("E2E command '{name}' field 'state' must not be empty");
    }

    let state = values
        .iter()
        .enumerate()
        .map(|(position, value)| parse_indexed_value(name, position, value))
        .collect::<anyhow::Result<Vec<_>>>()?;

    Ok(E2eStateParams { entity, state })
}

fn parse_subscribe_params(name: &str, params: &Value) -> anyhow::Result<E2eSubscribeParams> {
    let map = params_mapping(name, params)?;
    let channels = map
        .get(Value::String("channels".to_string()))
        .ok_or_else(|| anyhow::anyhow!("E2E command '{name}' requires 'channels'"))?
        .as_sequence()
        .ok_or_else(|| anyhow::anyhow!("E2E command '{name}' field 'channels' must be a list"))?
        .iter()
        .enumerate()
        .map(|(index, value)| {
            value.as_str().map(str::to_owned).ok_or_else(|| {
                anyhow::anyhow!(
                    "E2E command '{name}' channel at index {index} must be a string"
                )
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    if channels.is_empty() {
        anyhow::bail!("E2E command '{name}' requires at least one channel");
    }

    Ok(E2eSubscribeParams { channels })
}

fn parse_expect_params(name: &str, params: &Value) -> anyhow::Result<E2eExpectParams> {
    let map = params_mapping(name, params)?;
    let entity = required_string(map, "entity", name)?;
    let event = map
        .get(Value::String("event".to_string()))
        .ok_or_else(|| anyhow::anyhow!("E2E command '{name}' requires 'event'"))?
        .clone();
    if !event.is_mapping() {
        anyhow::bail!("E2E command '{name}' field 'event' must be a mapping");
    }

    if let Some(state) = event.get(Value::String("state".to_string())) {
        let values = state.as_sequence().ok_or_else(|| {
            anyhow::anyhow!("E2E command '{name}' event field 'state' must be a list")
        })?;
        for (position, value) in values.iter().enumerate() {
            parse_indexed_value(name, position, value)?;
        }
    }

    Ok(E2eExpectParams { entity, event })
}

fn params_mapping<'a>(name: &str, params: &'a Value) -> anyhow::Result<&'a Mapping> {
    params
        .as_mapping()
        .ok_or_else(|| anyhow::anyhow!("E2E command '{name}' requires a mapping"))
}

fn required_string(map: &Mapping, field: &str, name: &str) -> anyhow::Result<String> {
    let value = map
        .get(Value::String(field.to_string()))
        .ok_or_else(|| anyhow::anyhow!("E2E command '{name}' requires '{field}'"))?;
    let value = value
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("E2E command '{name}' field '{field}' must be a string"))?;
    if value.trim().is_empty() {
        anyhow::bail!("E2E command '{name}' field '{field}' must not be empty");
    }
    if field == "entity" && value.parse::<u64>().is_err() {
        anyhow::bail!(
            "E2E command '{name}' field '{field}' must be a decimal u64 string"
        );
    }
    Ok(value.to_string())
}

fn ensure_no_params(name: &str, params: &Value) -> anyhow::Result<()> {
    if params.is_null() || params.as_mapping().is_some_and(Mapping::is_empty) {
        return Ok(());
    }
    anyhow::bail!("E2E command '{name}' must not accept parameters")
}

fn parse_indexed_value(
    name: &str,
    position: usize,
    value: &Value,
) -> anyhow::Result<E2eIndexedValue> {
    let map = value.as_mapping().ok_or_else(|| {
        anyhow::anyhow!(
            "E2E command '{name}' state item at position {position} must be a mapping"
        )
    })?;
    let idx_value = map
        .get(Value::String("idx".to_string()))
        .ok_or_else(|| anyhow::anyhow!("E2E command '{name}' state item requires 'idx'"))?;
    let idx_number = idx_value.as_u64().ok_or_else(|| {
        anyhow::anyhow!(
            "E2E command '{name}' state item 'idx' must be an explicit non-negative integer"
        )
    })?;
    let state_value = map
        .get(Value::String("value".to_string()))
        .ok_or_else(|| anyhow::anyhow!("E2E command '{name}' state item requires 'value'"))?;

    Ok(E2eIndexedValue {
        idx: idx_number,
        value: state_value.clone(),
    })
}
