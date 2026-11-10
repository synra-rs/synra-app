use super::*;

/// Runs the startup probes and answers them, writing nothing.
///
/// # Why the ports are zero
///
/// The same reason the CLI gives: port zero asks the operating system for a free one, so the probes answer "is there a
/// free port on this machine" rather than "is the port we already hold busy" — the latter would report our own listener
/// as the fault.
///
/// # Why the host facts are empty
///
/// The window has no handle here for its WebView version or the platform keychain, so those probes answer `unknown` and
/// say so, exactly as they do for `synra doctor`. A fabricated fact would be worse than the missing one: the report is
/// something a maintainer reads as evidence.
pub fn diag_probes(state: &AppState) -> Result<ProbeReportDto, IpcError> {
    let targets = ProbeTargets {
        data_dir: state.paths.data_dir().to_path_buf(),
        plugins_dir: state.paths.plugins_dir(),
        listen_port: 0,
        discovery_port: 0,
    };
    let raw = run_probes(&targets, &HostFacts::default());
    Ok(report_dto(&raw))
}

/// The interface's view of a probe report.
fn report_dto(report: &ProbeReport) -> ProbeReportDto {
    ProbeReportDto {
        results: report.results.iter().map(result_dto).collect(),
    }
}

/// One probe result, flattened to the contract's strings.
fn result_dto(result: &synra_observe::doctor::ProbeResult) -> ProbeResultDto {
    ProbeResultDto {
        name: result.name.clone(),
        status: result.status.as_str().to_owned(),
        detail: result.detail.clone(),
        suggestion: result.suggestion.clone(),
    }
}
