//! Identity verification by emoji comparison (Matrix SAS), and cross-signing setup.
//!
//! The SDK runs the cryptographic protocol; this module watches each verification's state
//! stream, drives the steps that need no human (starting SAS once a request is accepted,
//! accepting SAS on the receiving side), and reports each stage to the session as a
//! [`VerificationUpdate`]. The person decides the rest: accept, then compare the seven emoji
//! with the other person and confirm or reject.

use std::collections::HashMap;

use chatcraft_core::UserId;
use chatcraft_engine::proto::{VerificationUpdate, VerifyAction, VerifyStage};
use futures_util::StreamExt;
use matrix_sdk::Client;
use matrix_sdk::encryption::verification::{SasState, SasVerification, Verification, VerificationRequest, VerificationRequestState};
use matrix_sdk::ruma::api::client::uiaa;
use tokio::sync::mpsc as tmpsc;

/// A state change from one of the watcher tasks.
pub(crate) enum Change {
    Request(String, VerificationRequestState),
    Sas(String, SasState),
}

struct Tracked {
    request: VerificationRequest,
    sas: Option<SasVerification>,
    user: UserId,
    mxid: String,
    incoming: bool,
    last: Option<VerifyStage>,
}

/// All verifications this device is part of.
pub(crate) struct Verifications {
    flows: HashMap<String, Tracked>,
    tx: tmpsc::UnboundedSender<Change>,
}

/// What happened, for the worker to report.
pub(crate) struct Outcome {
    pub update: Option<VerificationUpdate>,
    /// `(mxid, verified)` when an identity's trust changed.
    pub verified: Option<(String, bool)>,
}

impl Verifications {
    pub fn new(tx: tmpsc::UnboundedSender<Change>) -> Verifications {
        Verifications { flows: HashMap::new(), tx }
    }

    pub fn knows(&self, flow: &str) -> bool {
        self.flows.contains_key(flow)
    }

    /// Starts tracking a request (ours or theirs) and reports its first stage.
    pub fn track(&mut self, request: VerificationRequest, user: UserId) -> Option<VerificationUpdate> {
        let flow = request.flow_id().to_string();
        if self.flows.contains_key(&flow) {
            return None;
        }
        let incoming = !request.we_started();
        let tx = self.tx.clone();
        let (f, r) = (flow.clone(), request.clone());
        crate::rt::spawn(async move {
            let mut changes = r.changes();
            while let Some(state) = changes.next().await {
                if tx.send(Change::Request(f.clone(), state)).is_err() {
                    break;
                }
            }
        });
        let mxid = request.other_user_id().to_string();
        self.flows.insert(flow.clone(), Tracked { request, sas: None, user, mxid, incoming, last: None });
        self.stage(&flow, if incoming { VerifyStage::Requested } else { VerifyStage::Waiting })
    }

    fn watch_sas(&mut self, flow: &str, sas: SasVerification) {
        let tx = self.tx.clone();
        let (f, s) = (flow.to_string(), sas.clone());
        crate::rt::spawn(async move {
            let mut changes = s.changes();
            while let Some(state) = changes.next().await {
                if tx.send(Change::Sas(f.clone(), state)).is_err() {
                    break;
                }
            }
        });
        if let Some(t) = self.flows.get_mut(flow) {
            t.sas = Some(sas);
        }
    }

    /// Records a stage; returns an update only when it changed.
    fn stage(&mut self, flow: &str, stage: VerifyStage) -> Option<VerificationUpdate> {
        let t = self.flows.get_mut(flow)?;
        if t.last.as_ref() == Some(&stage) || t.last.as_ref().is_some_and(VerifyStage::finished) {
            return None;
        }
        t.last = Some(stage.clone());
        Some(VerificationUpdate { flow: flow.to_string(), user: t.user, incoming: t.incoming, stage })
    }

    /// Handles a state change from a watcher.
    pub async fn on_change(&mut self, change: Change) -> Outcome {
        let mut verified = None;
        let update = match change {
            Change::Request(flow, state) => match state {
                VerificationRequestState::Ready { .. } => {
                    let Some(t) = self.flows.get(&flow) else { return Outcome { update: None, verified } };
                    // The side that asked starts the emoji comparison.
                    if t.request.we_started() && t.sas.is_none() {
                        match t.request.start_sas().await {
                            Ok(Some(sas)) => self.watch_sas(&flow, sas),
                            Ok(None) => {
                                return Outcome {
                                    update: self.stage(&flow, VerifyStage::Cancelled { reason: "the other device can't compare emoji".into() }),
                                    verified,
                                };
                            }
                            Err(e) => return Outcome { update: self.stage(&flow, VerifyStage::Cancelled { reason: e.to_string() }), verified },
                        }
                    }
                    self.stage(&flow, VerifyStage::Waiting)
                }
                VerificationRequestState::Transitioned { verification: Verification::SasV1(sas), .. } => {
                    let new = self.flows.get(&flow).is_some_and(|t| t.sas.is_none());
                    if new {
                        // They started the comparison after we accepted: go along with it.
                        if !sas.we_started()
                            && let Err(e) = sas.accept().await
                        {
                            return Outcome { update: self.stage(&flow, VerifyStage::Cancelled { reason: e.to_string() }), verified };
                        }
                        self.watch_sas(&flow, sas);
                    }
                    None
                }
                VerificationRequestState::Done => {
                    verified = self.flows.get(&flow).map(|t| (t.mxid.clone(), true));
                    self.stage(&flow, VerifyStage::Done)
                }
                VerificationRequestState::Cancelled(info) => self.stage(&flow, VerifyStage::Cancelled { reason: info.reason().to_string() }),
                _ => None,
            },
            Change::Sas(flow, state) => match state {
                SasState::KeysExchanged { .. } => {
                    let emoji = self.flows.get(&flow).and_then(|t| t.sas.as_ref()).and_then(SasVerification::emoji);
                    match emoji {
                        Some(e) => self.stage(
                            &flow,
                            VerifyStage::Emoji { emoji: e.iter().map(|x| (x.symbol.to_string(), x.description.to_string())).collect() },
                        ),
                        None => self.stage(&flow, VerifyStage::Cancelled { reason: "the other device can't show emoji".into() }),
                    }
                }
                SasState::Confirmed => self.stage(&flow, VerifyStage::Confirmed),
                SasState::Done { .. } => {
                    verified = self.flows.get(&flow).map(|t| (t.mxid.clone(), true));
                    self.stage(&flow, VerifyStage::Done)
                }
                SasState::Cancelled(info) => self.stage(&flow, VerifyStage::Cancelled { reason: info.reason().to_string() }),
                _ => None,
            },
        };
        Outcome { update, verified }
    }

    /// The person's answer.
    pub async fn act(&mut self, flow: &str, action: VerifyAction) -> Result<Option<VerificationUpdate>, String> {
        let t = self.flows.get(flow).ok_or_else(|| format!("no verification `{flow}`"))?;
        let (request, sas) = (t.request.clone(), t.sas.clone());
        match action {
            VerifyAction::Accept => {
                request.accept().await.map_err(|e| e.to_string())?;
                Ok(self.stage(flow, VerifyStage::Waiting))
            }
            VerifyAction::Confirm => {
                let sas = sas.ok_or("there are no emoji to confirm yet")?;
                sas.confirm().await.map_err(|e| e.to_string())?;
                Ok(self.stage(flow, VerifyStage::Confirmed))
            }
            VerifyAction::Mismatch => {
                let sas = sas.ok_or("there are no emoji to compare yet")?;
                sas.mismatch().await.map_err(|e| e.to_string())?;
                Ok(self.stage(flow, VerifyStage::Cancelled { reason: "the emoji didn't match".into() }))
            }
            VerifyAction::Cancel => {
                match sas {
                    Some(s) => s.cancel().await.map_err(|e| e.to_string())?,
                    None => request.cancel().await.map_err(|e| e.to_string())?,
                }
                Ok(self.stage(flow, VerifyStage::Cancelled { reason: "cancelled".into() }))
            }
        }
    }
}

/// Sets up cross-signing for this account if it has none (needs the password to re-authenticate).
pub(crate) async fn bootstrap_cross_signing(client: &Client, user: &str, password: &str) -> Result<(), String> {
    let enc = client.encryption();
    match enc.bootstrap_cross_signing_if_needed(None).await {
        Ok(()) => Ok(()),
        Err(e) => {
            let Some(uiaa_info) = e.as_uiaa_response() else { return Err(e.to_string()) };
            let mut pw = uiaa::Password::new(
                uiaa::UserIdentifier::Matrix(uiaa::MatrixUserIdentifier::new(
                    user.trim_start_matches('@').split(':').next().unwrap_or(user).to_string(),
                )),
                password.to_string(),
            );
            pw.session = uiaa_info.session.clone();
            enc.bootstrap_cross_signing(Some(uiaa::AuthData::Password(pw))).await.map_err(|e| e.to_string())
        }
    }
}
