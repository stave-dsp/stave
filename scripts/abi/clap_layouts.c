#include "clap/clap.h"
#include <stdio.h>
#define S(T) printf("%s %zu %zu\n", #T, sizeof(T), _Alignof(T));
int main(){ S(clap_version_t) S(clap_host_t) S(clap_plugin_descriptor_t) S(clap_plugin_t) S(clap_audio_buffer_t) S(clap_process_t)
 S(clap_event_header_t) S(clap_event_note_t) S(clap_event_param_value_t) S(clap_event_transport_t) S(clap_input_events_t) S(clap_output_events_t)
 S(clap_plugin_factory_t) S(clap_plugin_entry_t) S(clap_audio_port_info_t) S(clap_plugin_audio_ports_t) S(clap_audio_ports_config_t)
 S(clap_plugin_audio_ports_config_t) S(clap_param_info_t) S(clap_plugin_params_t) S(clap_istream_t) S(clap_ostream_t) S(clap_plugin_state_t)
 S(clap_event_param_gesture_t) S(clap_plugin_tail_t) S(clap_plugin_latency_t) S(clap_host_log_t)
 S(clap_plugin_timer_support_t) S(clap_host_timer_support_t) S(clap_plugin_posix_fd_support_t) S(clap_host_posix_fd_support_t) }
