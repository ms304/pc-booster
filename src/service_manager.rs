use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::GetLastError;
use windows::Win32::System::Services::{
    CloseServiceHandle, ControlService, EnumServicesStatusExW,
    OpenSCManagerW, OpenServiceW, QueryServiceConfig2W, QueryServiceStatusEx, StartServiceW,
    SC_MANAGER_CONNECT, SC_MANAGER_ENUMERATE_SERVICE, SC_MANAGER_ALL_ACCESS,
    SERVICE_ACCEPT_STOP, SERVICE_ALL_ACCESS, SERVICE_CONTROL_STOP,
    SERVICE_RUNNING, SERVICE_START_PENDING, SERVICE_STOPPED,
    SERVICE_STOP_PENDING, SC_HANDLE, SC_ENUM_PROCESS_INFO, SERVICE_STATUS, SERVICE_STATUS_PROCESS,
    SERVICE_QUERY_STATUS, SERVICE_QUERY_CONFIG, ENUM_SERVICE_TYPE, ENUM_SERVICE_STATE,
    SC_STATUS_TYPE, SERVICE_CONFIG_DESCRIPTION,
};

#[derive(Debug, Clone)]
pub struct ServiceInfo {
    pub name: String,
    pub display_name: String,
    pub status: String,
    pub process_id: Option<u32>,
    pub description: Option<String>,
}

pub struct ServiceManager {
    sc_manager: SC_HANDLE,
}

// SAFETY: SC_HANDLE is a Windows service handle that can be safely used from any thread
unsafe impl Send for ServiceManager {}

impl ServiceManager {
    pub fn open() -> Result<Self, String> {
        unsafe {
            let sc_manager = OpenSCManagerW(
                PCWSTR::null(),
                PCWSTR::null(),
                SC_MANAGER_CONNECT | SC_MANAGER_ENUMERATE_SERVICE | SC_MANAGER_ALL_ACCESS,
            )
            .map_err(|e| format!("Failed to open service manager: {:?}", e))?;

            Ok(Self { sc_manager })
        }
    }

    pub fn list_services(&self) -> Result<Vec<ServiceInfo>, String> {
        unsafe {
            let mut services = Vec::new();
            let mut bytes_needed = 0u32;
            let mut services_returned = 0u32;
            let resume_handle: Option<*mut u32> = None;

            let mut buffer = Vec::new();

            loop {
                let result = EnumServicesStatusExW(
                    self.sc_manager,
                    SC_ENUM_PROCESS_INFO,
                    ENUM_SERVICE_TYPE(0x00000030), // SERVICE_WIN32
                    ENUM_SERVICE_STATE(0x00000003), // SERVICE_STATE_ALL
                    Some(&mut buffer),
                    &mut bytes_needed,
                    &mut services_returned,
                    resume_handle,
                    None,
                );

                if result.is_ok() {
                    break;
                }

                let err = GetLastError();
                if err.0 == 234 { // ERROR_MORE_DATA
                    buffer.resize(bytes_needed as usize, 0);
                    continue;
                }

                return Err(format!("Failed to enumerate services: {:?}", err));
            }

            let service_array = buffer.as_ptr() as *const ENUM_SERVICE_STATUS_PROCESSW;
            for i in 0..services_returned {
                let service = &*service_array.add(i as usize);
                let name = from_pcwstr(service.lpServiceName);
                let display_name = from_pcwstr(service.lpDisplayName);

                let status = match service.ServiceStatusProcess.dwCurrentState {
                    SERVICE_RUNNING => "Running".to_string(),
                    SERVICE_STOPPED => "Stopped".to_string(),
                    SERVICE_START_PENDING => "Starting".to_string(),
                    SERVICE_STOP_PENDING => "Stopping".to_string(),
                    _ => "Unknown".to_string(),
                };

                let process_id = if service.ServiceStatusProcess.dwCurrentState == SERVICE_RUNNING {
                    Some(service.ServiceStatusProcess.dwProcessId)
                } else {
                    None
                };

                services.push(ServiceInfo {
                    name,
                    display_name,
                    status,
                    process_id,
                    description: None, // Will be filled separately
                });
            }

            Ok(services)
        }
    }

    pub fn get_service_description(&self, name: &str) -> Result<String, String> {
        unsafe {
            let service_name = to_pcwstr(name);
            let service = OpenServiceW(
                self.sc_manager,
                PCWSTR(service_name.as_ptr()),
                SERVICE_QUERY_CONFIG,
            )
            .map_err(|e| format!("Failed to open service '{}': {:?}", name, e))?;

            let mut bytes_needed = 0u32;
            let result = QueryServiceConfig2W(
                service,
                SERVICE_CONFIG_DESCRIPTION,
                None,
                &mut bytes_needed,
            );

            if result.is_err() {
                let err = GetLastError();
                if err.0 != 122 { // ERROR_INSUFFICIENT_BUFFER
                    CloseServiceHandle(service);
                    return Err(format!("Failed to query service config: {:?}", err));
                }
            }

            let mut buffer = vec![0u8; bytes_needed as usize];
            let result = QueryServiceConfig2W(
                service,
                SERVICE_CONFIG_DESCRIPTION,
                Some(&mut buffer),
                &mut bytes_needed,
            );

            CloseServiceHandle(service);

            if result.is_err() {
                return Err(format!("Failed to get service description: {:?}", GetLastError()));
            }

            let description = &*(buffer.as_ptr() as *const SERVICE_DESCRIPTIONW);
            let desc_str = from_pcwstr(description.lpDescription);

            if desc_str.is_empty() {
                Ok("No description available".to_string())
            } else {
                Ok(desc_str)
            }
        }
    }

    pub fn stop_service(&self, name: &str) -> Result<(), String> {
        unsafe {
            let service_name = to_pcwstr(name);
            let service = OpenServiceW(
                self.sc_manager,
                PCWSTR(service_name.as_ptr()),
                SERVICE_ALL_ACCESS,
            )
            .map_err(|e| format!("Failed to open service '{}': {:?}", name, e))?;

            let mut status = SERVICE_STATUS_PROCESS::default();
            let mut bytes_needed = 0u32;
            let result = QueryServiceStatusEx(
                service,
                SC_STATUS_TYPE(0),
                Some(std::slice::from_raw_parts_mut(
                    &mut status as *mut _ as *mut u8,
                    std::mem::size_of::<SERVICE_STATUS_PROCESS>(),
                )),
                &mut bytes_needed,
            );

            if result.is_err() {
                CloseServiceHandle(service);
                return Err(format!("Failed to query service status: {:?}", GetLastError()));
            }

            if status.dwCurrentState == SERVICE_STOPPED {
                CloseServiceHandle(service);
                return Ok(());
            }

            if status.dwControlsAccepted & SERVICE_ACCEPT_STOP == 0 {
                CloseServiceHandle(service);
                return Err(format!("Service '{}' does not accept stop commands", name));
            }

            let mut service_status = SERVICE_STATUS::default();
            let result = ControlService(service, SERVICE_CONTROL_STOP, &mut service_status);
            if result.is_err() {
                CloseServiceHandle(service);
                return Err(format!("Failed to stop service '{}': {:?}", name, GetLastError()));
            }

            CloseServiceHandle(service);
            Ok(())
        }
    }

    pub fn start_service(&self, name: &str) -> Result<(), String> {
        unsafe {
            let service_name = to_pcwstr(name);
            let service = OpenServiceW(
                self.sc_manager,
                PCWSTR(service_name.as_ptr()),
                SERVICE_ALL_ACCESS,
            )
            .map_err(|e| format!("Failed to open service '{}': {:?}", name, e))?;

            let result = StartServiceW(service, None);
            CloseServiceHandle(service);

            if result.is_err() {
                Err(format!("Failed to start service '{}': {:?}", name, GetLastError()))
            } else {
                Ok(())
            }
        }
    }

    pub fn is_service_running(&self, name: &str) -> Result<bool, String> {
        unsafe {
            let service_name = to_pcwstr(name);
            let service = OpenServiceW(
                self.sc_manager,
                PCWSTR(service_name.as_ptr()),
                SERVICE_QUERY_STATUS,
            )
            .map_err(|e| format!("Failed to open service '{}': {:?}", name, e))?;

            let mut status = SERVICE_STATUS_PROCESS::default();
            let mut bytes_needed = 0u32;
            let result = QueryServiceStatusEx(
                service,
                SC_STATUS_TYPE(0),
                Some(std::slice::from_raw_parts_mut(
                    &mut status as *mut _ as *mut u8,
                    std::mem::size_of::<SERVICE_STATUS_PROCESS>(),
                )),
                &mut bytes_needed,
            );

            CloseServiceHandle(service);

            if result.is_err() {
                Err(format!("Failed to query service status: {:?}", GetLastError()))
            } else {
                Ok(status.dwCurrentState == SERVICE_RUNNING)
            }
        }
    }
}

impl Drop for ServiceManager {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseServiceHandle(self.sc_manager);
        }
    }
}

// Windows API structures
#[repr(C)]
struct ENUM_SERVICE_STATUS_PROCESSW {
    lpServiceName: PWSTR,
    lpDisplayName: PWSTR,
    ServiceStatusProcess: SERVICE_STATUS_PROCESS,
}

#[repr(C)]
struct SERVICE_DESCRIPTIONW {
    lpDescription: PWSTR,
}

fn to_pcwstr(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn from_pcwstr(ptr: PWSTR) -> String {
    unsafe {
        let mut len = 0;
        while *ptr.0.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(ptr.0, len))
    }
}
