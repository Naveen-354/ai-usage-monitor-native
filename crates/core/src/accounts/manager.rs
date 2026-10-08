//! The operations on accounts - sign in, list, switch, check, remove, launch - written once and used by both `agm` and the
//! desktop UI, which therefore always see and change the same state.
//!
//! What "switching" means here, and why it is safe: the active account is a row in the shared database. Choosing another
//! one changes that row (inside a lock and a transaction) and nothing else - no agent file is rewritten, no sign-in is
//! moved. The choice takes effect when the agent is *launched* through `agm run`, which points it at that account's own
//! profile folder with the agent's documented environment variable. An agent that is already running keeps the account it
//! started with.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use super::error::{AccountError, AccountResult};
use super::runner::SystemRunner;
use super::secrets::KeyringSecretStore;
use crate::aggregation::{account_usage, AccountUsage};
use crate::database::Database;
use crate::settings::SettingsStore;
use super::lock::{AgentLock, DEFAULT_WAIT};
use super::model::{Account, AccountKind, AuthState};
use super::provider::{AccountProvider, LoginMethod, Observation, ProfileDirs, SwitchSupport};
use super::providers::{by_id, registry, SharedProvider};
use super::runner::{is_inside, resolve_executable, ProcessRunner, ProcessSpec};
use super::secrets::{Secret, SecretKey, SecretStore};
use super::store::AccountStore;
use crate::collectors::Env;
use crate::model::{catalog, DEFAULT_ACCOUNT};

/// What we say about an account that runs on an API key we hold.
const API_KEY_NOTE: &str = "API key stored in the OS credential store; it is not checked until used";

/// How long a sign-in may stay unfinished before start-up treats it as abandoned. A sign-in that is genuinely in progress in
/// another window (or another process) is "pending" too, and must never be cleaned up from under the person doing it.
const PENDING_GRACE: Duration = Duration::from_secs(30 * 60);

/// How long to wait for an agent's status command.
const STATUS_TIMEOUT: Duration = Duration::from_secs(20);

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// One agent with its accounts, as the CLI and the UI show it: Agent -> Accounts -> Active account.
#[derive(Debug, Clone)]
pub struct AgentAccounts {
    pub agent_id: String,
    pub name: String,
    /// Is the agent's executable on this machine?
    pub installed: bool,
    pub binary: Option<PathBuf>,
    pub switch: SwitchSupport,
    pub login_methods: Vec<LoginMethod>,
    /// The id of the account in use.
    pub active: Option<String>,
    pub accounts: Vec<Account>,
}

impl AgentAccounts {
    pub fn active_account(&self) -> Option<&Account> {
        self.active.as_ref().and_then(|id| self.accounts.iter().find(|a| &a.account_id == id))
    }
}

/// The result of choosing an account.
#[derive(Debug, Clone)]
pub struct UseOutcome {
    pub account: Account,
    /// Something the user should know (e.g. the account's sign-in has expired). The switch still happened.
    pub warning: Option<String>,
}

/// A sign-in that has been prepared (the account exists, its folder is ready) and is about to run - or has run. Holds the
/// command so the caller decides *where* it runs: the user's terminal, or a window of its own.
pub struct LoginPlan {
    pub account: Account,
    pub method: LoginMethod,
    /// `None` when there is no command to run (an API key we keep ourselves).
    pub command: Option<ProcessSpec>,
    /// Did this call create the account (so a failed sign-in removes it again)?
    pub fresh: bool,
    api_key: Option<Secret>,
}

impl std::fmt::Debug for LoginPlan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginPlan")
            .field("account", &self.account.display_ref())
            .field("method", &self.method)
            .field("command", &self.command)
            .field("fresh", &self.fresh)
            .field("api_key", &self.api_key.as_ref().map(|_| "[redacted]"))
            .finish()
    }
}

pub struct AccountManager {
    store: AccountStore,
    providers: std::collections::HashMap<&'static str, SharedProvider>,
    runner: Arc<dyn ProcessRunner>,
    secrets: Arc<dyn SecretStore>,
    env: Env,
    data_dir: PathBuf,
    lock_wait: Duration,
    pending_grace: Duration,
}

impl AccountManager {
    pub fn new(store: AccountStore, runner: Arc<dyn ProcessRunner>, secrets: Arc<dyn SecretStore>, env: Env, data_dir: PathBuf) -> AccountManager {
        AccountManager::with_providers(store, registry(), runner, secrets, env, data_dir)
    }

    pub fn with_providers(
        store: AccountStore,
        providers: Vec<SharedProvider>,
        runner: Arc<dyn ProcessRunner>,
        secrets: Arc<dyn SecretStore>,
        env: Env,
        data_dir: PathBuf,
    ) -> AccountManager {
        AccountManager { store, providers: by_id(providers), runner, secrets, env, data_dir, lock_wait: DEFAULT_WAIT, pending_grace: PENDING_GRACE }
    }

    /// For tests: how old an unfinished sign-in must be before start-up settles it.
    pub fn with_pending_grace(mut self, grace: Duration) -> AccountManager {
        self.pending_grace = grace;
        self
    }

    /// The manager over the real machine: the shared database in `data_dir`, the real process runner and the OS credential
    /// store. Starts no background work, so the CLI can use it next to a running app. Calls [`init`](Self::init).
    pub fn open(data_dir: &Path) -> AccountResult<AccountManager> {
        std::fs::create_dir_all(data_dir)?;
        let db = Database::open(&data_dir.join("usage.db"))?;
        let manager = AccountManager::new(AccountStore::new(db), Arc::new(SystemRunner), Arc::new(KeyringSecretStore), Env::from_system(), data_dir.to_path_buf());
        manager.init()?;
        Ok(manager)
    }

    /// Usage of every account over today / this week / this month / this year / lifetime, from the shared database.
    pub fn usage(&self) -> AccountResult<Vec<AccountUsage>> {
        let settings = SettingsStore::load(self.store.database().clone())?.get();
        Ok(self.store.database().with_reader(|c| account_usage(c, &settings, &chrono::Local, chrono::Utc::now()))?)
    }

    /// For tests: give up waiting for a lock sooner.
    pub fn with_lock_wait(mut self, wait: Duration) -> AccountManager {
        self.lock_wait = wait;
        self
    }

    pub fn store(&self) -> &AccountStore {
        &self.store
    }

    /// The folder every managed account's profile lives under. Nothing outside it is ever created or deleted.
    pub fn accounts_root(&self) -> PathBuf {
        self.data_dir.join("accounts")
    }

    /// Makes sure every agent has its default account, and settles any sign-in that was interrupted (the app or the CLI was
    /// closed half-way): one that did complete is kept, one that did not is cleaned up. Safe to call at every start.
    pub fn init(&self) -> AccountResult<()> {
        let ids: Vec<&str> = catalog().iter().map(|a| a.id).collect();
        self.store.ensure_defaults(&ids, now_ms())?;
        self.recover_pending()
    }

    fn provider(&self, agent: &str) -> AccountResult<&SharedProvider> {
        self.providers.get(agent).ok_or_else(|| AccountError::UnknownAgent(agent.to_string()))
    }

    fn binary(&self, p: &dyn AccountProvider) -> Option<PathBuf> {
        resolve_executable(&self.env, p.binary_names(), &p.extra_search_dirs(&self.env))
    }

    fn lock(&self, agent: &str) -> AccountResult<AgentLock> {
        AgentLock::acquire(&self.data_dir, agent, self.lock_wait)
    }

    fn profile_dir_for(&self, agent: &str, id: &str) -> PathBuf {
        self.accounts_root().join(agent).join(id)
    }

    /// Is `dir` somewhere under the accounts folder, with no `..` to climb out of it? (A path check, not a disk check, so it also
    /// works for a folder that does not exist yet.)
    fn is_managed_path(&self, dir: &Path) -> bool {
        dir.starts_with(self.accounts_root()) && dir.components().all(|c| c != std::path::Component::ParentDir)
    }

    /// Makes sure the folders an agent is pointed at exist before it is started or asked anything. Codex, for one, refuses to
    /// run when `CODEX_HOME` names a folder that is not there. Only ever creates folders inside the accounts folder.
    fn ensure_profile_dirs(&self, p: &dyn AccountProvider, account: &Account) {
        let Some(profile) = self.profile_of(account) else { return };
        if !account.profile_dir.as_deref().is_some_and(|d| self.is_managed_path(d)) {
            return;
        }
        let _ = std::fs::create_dir_all(&profile.home);
        for (_, v) in p.profile_env(&profile) {
            let _ = std::fs::create_dir_all(Path::new(&v));
        }
    }

    /// The profile of a managed account, from where the store says it is - refused unless it really is under our folder.
    fn profile_of(&self, account: &Account) -> Option<ProfileDirs> {
        let dir = account.profile_dir.as_ref()?;
        Some(ProfileDirs::new(dir))
    }

    // ---------------------------------------------------------------------------------------------------- listing

    pub fn agent(&self, agent: &str) -> AccountResult<AgentAccounts> {
        let p = self.provider(agent)?;
        let meta = catalog().iter().find(|a| a.id == agent).ok_or_else(|| AccountError::UnknownAgent(agent.to_string()))?;
        let caps = p.capabilities();
        let binary = self.binary(p.as_ref());
        let accounts = self.store.list(Some(agent), false)?;
        let active = self.store.active(agent)?.map(|a| a.account_id);
        Ok(AgentAccounts {
            agent_id: agent.to_string(),
            name: meta.name.to_string(),
            installed: binary.is_some(),
            binary,
            switch: caps.switch_support(),
            login_methods: caps.login_methods.to_vec(),
            active,
            accounts,
        })
    }

    /// Every agent, in catalogue order.
    pub fn agents(&self) -> AccountResult<Vec<AgentAccounts>> {
        catalog().iter().map(|a| self.agent(a.id)).collect()
    }

    // ---------------------------------------------------------------------------------------------------- signing in

    /// Prepares a new managed account and the sign-in command for it. Nothing has been signed in yet.
    pub fn plan_login(&self, agent: &str, label: &str, method: LoginMethod, api_key: Option<Secret>) -> AccountResult<LoginPlan> {
        let p = self.provider(agent)?;
        let caps = p.capabilities();
        if let SwitchSupport::Unavailable { reason } = caps.switch_support() {
            return Err(AccountError::SwitchingUnavailable { agent: agent.to_string(), reason: reason.to_string() });
        }
        self.check_method(agent, method, &api_key)?;
        let binary = self.binary(p.as_ref()).ok_or_else(|| AccountError::NotInstalled(agent.to_string()))?;

        let _lock = self.lock(agent)?;
        let account = self.store.insert_managed(agent, label, |id| self.profile_dir_for(agent, id), now_ms())?;
        match self.prepare(p.as_ref(), &account, &binary, method, &api_key) {
            Ok(command) => Ok(LoginPlan { account, method, command, fresh: true, api_key }),
            Err(e) => {
                self.discard(&account);
                Err(e)
            }
        }
    }

    /// Prepares a new sign-in for an account that already exists (its session expired, or it was never completed).
    pub fn plan_relogin(&self, agent: &str, account_ref: &str, method: LoginMethod, api_key: Option<Secret>) -> AccountResult<LoginPlan> {
        let p = self.provider(agent)?;
        self.check_method(agent, method, &api_key)?;
        let binary = self.binary(p.as_ref()).ok_or_else(|| AccountError::NotInstalled(agent.to_string()))?;
        let _lock = self.lock(agent)?;
        let account = self.store.find(agent, account_ref)?.ok_or_else(|| AccountError::UnknownAccount { agent: agent.into(), account: account_ref.into() })?;
        if account.is_default() {
            return Err(AccountError::Unsupported(format!(
                "the default account is {agent}'s own sign-in: sign in with {agent} itself, then run `agm accounts {agent} --check`"
            )));
        }
        let command = self.prepare(p.as_ref(), &account, &binary, method, &api_key)?;
        Ok(LoginPlan { account, method, command, fresh: false, api_key })
    }

    fn check_method(&self, agent: &str, method: LoginMethod, api_key: &Option<Secret>) -> AccountResult<()> {
        let caps = self.provider(agent)?.capabilities();
        if !caps.login_methods.contains(&method) {
            let offered: Vec<&str> = caps.login_methods.iter().map(|m| m.as_str()).collect();
            return Err(AccountError::Unsupported(format!("{agent} cannot sign in by '{}' (it offers: {})", method.as_str(), offered.join(", "))));
        }
        if method == LoginMethod::ApiKey && api_key.as_ref().is_none_or(Secret::is_empty) {
            return Err(AccountError::Unsupported("an API key is needed for this sign-in".into()));
        }
        Ok(())
    }

    /// Creates the profile folder and builds the command (or `None` when an API key is simply kept for launch).
    fn prepare(&self, p: &dyn AccountProvider, account: &Account, binary: &Path, method: LoginMethod, api_key: &Option<Secret>) -> AccountResult<Option<ProcessSpec>> {
        let profile = self.profile_of(account).ok_or_else(|| AccountError::Unsupported("the account has no profile folder".into()))?;
        std::fs::create_dir_all(&profile.home)?;
        let env = p.profile_env(&profile);
        for (_, v) in &env {
            std::fs::create_dir_all(Path::new(v))?; // the agent's own folder inside the profile
        }
        let Some(args) = p.login_args(method) else { return Ok(None) };
        let stdin = if p.login_reads_key_from_stdin(method) { api_key.as_ref().map(|k| format!("{}\n", k.expose()).into_bytes()) } else { None };
        Ok(Some(ProcessSpec {
            program: binary.to_path_buf(),
            args,
            env: env.into_iter().map(|(k, v)| (OsString::from(k), v)).collect(),
            env_remove: p.capabilities().api_key_env.map(|k| vec![OsString::from(k)]).unwrap_or_default(),
            cwd: None,
            stdin,
        }))
    }

    /// Settles a sign-in after its command ran: confirms with the agent that the account is signed in, keeps an API key if we
    /// are the ones holding it, and records the result. A sign-in that did not work leaves nothing behind.
    pub fn finish_login(&self, plan: LoginPlan, ran: io::Result<i32>) -> AccountResult<Account> {
        let agent = plan.account.agent_id.clone();
        let p = self.provider(&agent)?.clone();
        let _lock = self.lock(&agent)?;
        let fail = |this: &Self, why: String| -> AccountResult<Account> {
            if plan.fresh {
                this.discard(&plan.account);
            } else {
                let _ = this.store.set_auth(&agent, &plan.account.account_id, AuthState::NotLoggedIn, None, Some("the last sign-in did not complete"), now_ms());
            }
            Err(AccountError::LoginFailed(why))
        };
        if plan.command.is_some() {
            match ran {
                Ok(0) => {}
                Ok(code) => return fail(self, format!("{} exited with code {code}", p.binary_names()[0])),
                Err(e) => return fail(self, format!("could not start {}: {e}", p.binary_names()[0])),
            }
        }
        let keeps_key_itself = plan.method == LoginMethod::ApiKey && plan.command.is_none();
        if keeps_key_itself {
            let key = SecretKey::api_key(&agent, &plan.account.account_id);
            let Some(secret) = &plan.api_key else { return fail(self, "no API key was given".into()) };
            if let Err(e) = self.secrets.set(&key, secret) {
                return fail(self, e.to_string());
            }
        }
        // a key we keep ourselves has no sign-in to ask the agent about
        let obs = if keeps_key_itself { Observation::unknown() } else { self.observe(p.as_ref(), &plan.account) };
        let (state, detail, identity): (AuthState, Option<&str>, Option<String>) = match (keeps_key_itself, obs.signed_in) {
            (true, _) => (AuthState::Unknown, Some(API_KEY_NOTE), None),
            (false, Some(true)) => (AuthState::Valid, None, obs.identity),
            (false, Some(false)) => return fail(self, "the sign-in finished but the agent still reports that it is not signed in".into()),
            (false, None) => (AuthState::Unknown, Some("signed in, but the agent offers no way to confirm it"), None),
        };
        self.store.set_auth(&agent, &plan.account.account_id, state, identity.as_deref(), detail, now_ms())?;
        self.store.require(&agent, &plan.account.account_id)
    }

    /// Signs in on the user's own terminal and returns the account.
    pub fn login(&self, agent: &str, label: &str, method: LoginMethod, api_key: Option<Secret>) -> AccountResult<Account> {
        let plan = self.plan_login(agent, label, method, api_key)?;
        let ran = match &plan.command {
            Some(spec) => self.runner.run_attached(spec),
            None => Ok(0),
        };
        self.finish_login(plan, ran)
    }

    /// Signs in again on the user's own terminal.
    pub fn relogin(&self, agent: &str, account: &str, method: LoginMethod, api_key: Option<Secret>) -> AccountResult<Account> {
        let plan = self.plan_relogin(agent, account, method, api_key)?;
        let ran = match &plan.command {
            Some(spec) => self.runner.run_attached(spec),
            None => Ok(0),
        };
        self.finish_login(plan, ran)
    }

    /// Runs a prepared sign-in in a console window of its own (the desktop app's way) and settles it.
    pub fn run_login_in_new_window(&self, plan: LoginPlan) -> AccountResult<Account> {
        let ran = match &plan.command {
            Some(spec) => self.runner.run_in_new_window(spec),
            None => Ok(0),
        };
        self.finish_login(plan, ran)
    }

    /// Removes an account that was just created and never worked: its row, its folder, its stored key.
    fn discard(&self, account: &Account) {
        if let Some(dir) = &account.profile_dir {
            self.delete_profile_dir(dir);
        }
        let _ = self.secrets.delete(&SecretKey::api_key(&account.agent_id, &account.account_id));
        let _ = self.store.discard_pending(&account.agent_id, &account.account_id);
    }

    fn delete_profile_dir(&self, dir: &Path) -> bool {
        if !dir.exists() {
            return true;
        }
        if !is_inside(dir, &self.accounts_root()) {
            tracing::warn!("refusing to delete a folder outside the accounts folder");
            return false;
        }
        std::fs::remove_dir_all(dir).is_ok()
    }

    /// Settles sign-ins that were started and never finished (a crash, a closed window) - but only ones that have been pending for
    /// longer than any sign-in plausibly takes, so one in progress in another process is left alone.
    fn recover_pending(&self) -> AccountResult<()> {
        let now = now_ms();
        let grace = i64::try_from(self.pending_grace.as_millis()).unwrap_or(i64::MAX);
        let stale = |a: &Account| a.auth == AuthState::Pending && now.saturating_sub(a.created_utc_ms) >= grace;
        for account in self.store.list(None, false)?.into_iter().filter(stale) {
            let Ok(p) = self.provider(&account.agent_id) else { continue };
            let _lock = self.lock(&account.agent_id)?;
            match self.observe(p.as_ref(), &account).signed_in {
                Some(true) => self.store.set_auth(&account.agent_id, &account.account_id, AuthState::Valid, None, None, now_ms())?,
                _ => self.discard(&account),
            }
        }
        Ok(())
    }

    // ---------------------------------------------------------------------------------------------------- authentication state

    /// Asks the agent (with its own status command, or by looking for its sign-in file) whether the account is signed in.
    fn observe(&self, p: &dyn AccountProvider, account: &Account) -> Observation {
        let managed = account.kind == AccountKind::Managed;
        let profile = if managed { self.profile_of(account) } else { Some(ProfileDirs { home: self.env.home.clone() }) };
        if let Some(args) = p.status_args() {
            let Some(binary) = self.binary(p) else { return Observation::unknown() };
            let mut spec = ProcessSpec { program: binary, args, ..Default::default() };
            if managed {
                let Some(profile) = &profile else { return Observation::unknown() };
                self.ensure_profile_dirs(p, account);
                spec.env = p.profile_env(profile).into_iter().map(|(k, v)| (OsString::from(k), v)).collect();
                // an API key in the shell must not make a different account look signed in
                spec.env_remove = p.capabilities().api_key_env.map(|k| vec![OsString::from(k)]).unwrap_or_default();
            }
            return match self.runner.capture(&spec, STATUS_TIMEOUT) {
                Ok(out) => p.interpret_status(&out),
                Err(_) => Observation::unknown(),
            };
        }
        if let Some(profile) = &profile {
            let markers = p.credential_markers(profile);
            if !markers.is_empty() {
                // existence only: the file is never opened
                return if markers.iter().any(|m| m.is_file()) { Observation::signed_in(None) } else { Observation::signed_out() };
            }
        }
        Observation::unknown()
    }

    /// Re-checks one account and records what was found.
    pub fn check_auth(&self, agent: &str, account: &str) -> AccountResult<Account> {
        let p = self.provider(agent)?;
        let acc = self.store.require(agent, account)?;
        // An account that runs on an API key we hold has no sign-in to check: the agent would (rightly) say "not signed in".
        if acc.kind == AccountKind::Managed && p.capabilities().api_key_env.is_some() && self.secrets.get(&SecretKey::api_key(agent, account))?.is_some() {
            self.store.set_auth(agent, account, AuthState::Unknown, None, Some(API_KEY_NOTE), now_ms())?;
            return self.store.require(agent, account);
        }
        let obs = self.observe(p.as_ref(), &acc);
        let (state, detail) = next_state(acc.auth, &obs);
        self.store.set_auth(agent, account, state, obs.identity.as_deref(), detail, now_ms())?;
        self.store.require(agent, account)
    }

    /// Re-checks every account of every agent that can be checked. One that cannot be reached does not stop the rest.
    pub fn check_all(&self) -> Vec<(String, String, AccountResult<Account>)> {
        let mut out = Vec::new();
        for agent in catalog() {
            let Ok(p) = self.provider(agent.id) else { continue };
            if !p.capabilities().can_check_auth || self.binary(p.as_ref()).is_none() && p.status_args().is_some() {
                continue;
            }
            for a in self.store.list(Some(agent.id), false).unwrap_or_default() {
                if a.auth == AuthState::Pending {
                    continue;
                }
                out.push((a.agent_id.clone(), a.account_id.clone(), self.check_auth(&a.agent_id, &a.account_id)));
            }
        }
        out
    }

    // ---------------------------------------------------------------------------------------------------- choosing

    /// Makes an account the one the agent uses from now on (found by id or by name).
    pub fn use_account(&self, agent: &str, account_ref: &str) -> AccountResult<UseOutcome> {
        self.provider(agent)?;
        let _lock = self.lock(agent)?;
        let account = self
            .store
            .find(agent, account_ref)?
            .ok_or_else(|| AccountError::UnknownAccount { agent: agent.to_string(), account: account_ref.to_string() })?;
        self.store.set_active(agent, &account.account_id, now_ms())?;
        let warning = match account.auth {
            AuthState::Expired => Some(format!("{} has expired; sign in again with `agm login {agent} --account {}`", account.label, account.account_id)),
            AuthState::NotLoggedIn | AuthState::Pending => Some(format!("{} is not signed in; sign in with `agm login {agent} --account {}`", account.label, account.account_id)),
            _ => None,
        };
        Ok(UseOutcome { account, warning })
    }

    /// The account each agent is using now.
    pub fn current(&self) -> AccountResult<Vec<(String, Account)>> {
        let mut out = Vec::new();
        for agent in catalog() {
            if let Some(a) = self.store.active(agent.id)? {
                out.push((agent.id.to_string(), a));
            }
        }
        Ok(out)
    }

    // ---------------------------------------------------------------------------------------------------- removing

    /// Forgets an account: its sign-in folder, any key we hold, and its place in the lists. Its usage history stays unless
    /// `purge_usage`. The default account cannot be removed.
    pub fn remove(&self, agent: &str, account_ref: &str, purge_usage: bool) -> AccountResult<()> {
        self.provider(agent)?;
        let _lock = self.lock(agent)?;
        let account = self
            .store
            .find(agent, account_ref)?
            .ok_or_else(|| AccountError::UnknownAccount { agent: agent.to_string(), account: account_ref.to_string() })?;
        if account.is_default() {
            return Err(AccountError::CannotRemoveDefault);
        }
        // Delete the sign-in first: if its folder is in use we stop here and the account is still listed.
        if let Some(dir) = &account.profile_dir {
            if dir.exists() && !self.delete_profile_dir(dir) {
                return Err(AccountError::Unsupported(format!(
                    "could not delete the account's folder (is an agent session still using it?): {}",
                    dir.display()
                )));
            }
        }
        self.secrets.delete(&SecretKey::api_key(agent, &account.account_id))?;
        let dir = self.store.soft_remove(agent, &account.account_id, now_ms())?;
        if purge_usage {
            self.store.purge_usage(agent, &account.account_id, dir.as_deref().and_then(|d| d.to_str()))?;
        }
        Ok(())
    }

    // ---------------------------------------------------------------------------------------------------- launching

    /// The command that starts the agent with its active account: the profile's environment for a managed account, an API
    /// key (from the OS credential store, for this launch only) where one is kept, and an API key *removed* from the
    /// environment where it would otherwise override the account.
    pub fn launch_spec(&self, agent: &str, args: &[OsString]) -> AccountResult<(ProcessSpec, Account)> {
        let p = self.provider(agent)?;
        let binary = self.binary(p.as_ref()).ok_or_else(|| AccountError::NotInstalled(agent.to_string()))?;
        let account = self.store.active(agent)?.ok_or_else(|| AccountError::UnknownAccount { agent: agent.into(), account: DEFAULT_ACCOUNT.into() })?;
        if !account.auth.is_usable() {
            return Err(AccountError::NeedsLogin(format!("{agent}/{}", account.account_id)));
        }
        let mut spec = ProcessSpec { program: binary, args: args.to_vec(), ..Default::default() };
        if account.kind == AccountKind::Managed {
            let profile = self.profile_of(&account).ok_or_else(|| AccountError::NeedsLogin(format!("{agent}/{}", account.account_id)))?;
            self.ensure_profile_dirs(p.as_ref(), &account);
            spec.env = p.profile_env(&profile).into_iter().map(|(k, v)| (OsString::from(k), v)).collect();
            if let Some(var) = p.capabilities().api_key_env {
                match self.secrets.get(&SecretKey::api_key(agent, &account.account_id))? {
                    Some(key) => spec.env.push((OsString::from(var), OsString::from(key.expose()))),
                    None => spec.env_remove.push(OsString::from(var)),
                }
            }
        }
        self.store.touch_used(agent, &account.account_id, now_ms())?;
        Ok((spec, account))
    }

    /// Starts the agent with its active account on the user's terminal and returns its exit code.
    pub fn run(&self, agent: &str, args: &[OsString]) -> AccountResult<i32> {
        let (spec, _) = self.launch_spec(agent, args)?;
        Ok(self.runner.run_attached(&spec)?)
    }
}

/// How an observation changes what we believe about an account.
fn next_state(prev: AuthState, obs: &Observation) -> (AuthState, Option<&'static str>) {
    match obs.signed_in {
        Some(true) => (AuthState::Valid, None),
        Some(false) => match prev {
            // it was fine before and now is not: expired or revoked, as opposed to never having been signed in
            AuthState::Valid | AuthState::Expired => (AuthState::Expired, Some("it was signed in and the agent now reports no sign-in: sign in again")),
            _ => (AuthState::NotLoggedIn, Some("the agent reports no sign-in")),
        },
        None => (prev, Some("could not check right now")),
    }
}

/// The environment a collector uses for each managed account: `(agent, account id, env)`. Used by the monitor, which
/// therefore reads usage from every account's own folder.
pub fn account_collector_envs(store: &AccountStore, system: &Env) -> Vec<(String, String, Env)> {
    store
        .list(None, false)
        .unwrap_or_default()
        .into_iter()
        .filter(|a| a.kind == AccountKind::Managed)
        .filter_map(|a| {
            let profile = ProfileDirs::new(a.profile_dir.as_ref()?);
            Some((a.agent_id, a.account_id, super::provider::account_env(system, &profile)))
        })
        .collect()
}

#[cfg(test)]
mod tests;
