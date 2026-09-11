// Thin typed wrappers over the Tauri command layer. Every backend call in
// the app goes through here so command names live in exactly one place.
import { invoke } from '@tauri-apps/api/core';

const call = (cmd, args) => invoke(cmd, args);

export const api = {
  // window / environment
  windowEnvironment: () => call('window_environment'),

  // connections
  connectionsList: () => call('connections_list'),
  connectionSave: (config, password) => call('connection_save', { config, password }),
  connectionDelete: (id) => call('connection_delete', { id }),
  connectionsReorder: (ids) => call('connections_reorder', { ids }),
  connectionTest: (config, password) => call('connection_test', { config, password }),

  // schema
  schemasList: (config) => call('schemas_list', { config }),
  columnsList: (config, schema, relation) => call('columns_list', { config, schema, relation }),
  foreignKeysList: (config) => call('foreign_keys_list', { config }),

  // query — page/pageSize omitted = full result (capped by max rows)
  queryRun: (config, sql, page = null, pageSize = null, count = null) =>
    call('query_run', { config, sql, page, pageSize, count }),

  // tabs
  tabsListAll: () => call('tabs_list_all'),
  tabSave: (tab) => call('tab_save', { tab }),
  tabDelete: (id) => call('tab_delete', { id }),

  // history
  historyRecent: (limit) => call('history_recent', { limit }),
  historyRequestRecent: (limit) => call('history_request_recent', { limit }),
  historyResult: (id) => call('history_result', { id }),
  historyRequestResult: (id) => call('history_request_result', { id }),
  queryLimitsGet: () => call('query_limits_get'),
  queryLimitsSet: (maxRows) => call('query_limits_set', { maxRows }),

  // schedules
  schedulesList: () => call('schedules_list'),
  scheduleSave: (schedule) => call('schedule_save', { schedule }),
  scheduleDelete: (id) => call('schedule_delete', { id }),
  scheduleRunNow: (id) => call('schedule_run_now', { id }),

  // saved queries
  savedQueriesList: () => call('saved_queries_list'),
  savedQuerySave: (query) => call('saved_query_save', { query }),
  savedQueryDelete: (id) => call('saved_query_delete', { id }),
  savedQueryFoldersList: () => call('saved_query_folders_list'),
  savedQueryFolderSave: (folder) => call('saved_query_folder_save', { folder }),
  savedQueryFolderDelete: (id) => call('saved_query_folder_delete', { id }),
  savedChartsList: () => call('saved_charts_list'),
  savedChartData: (id) => call('saved_chart_data', { id }),
  savedChartSave: (chart) => call('saved_chart_save', { chart }),
  savedChartDelete: (id) => call('saved_chart_delete', { id }),

  // request collections
  collectionsList: () => call('collections_list'),
  collectionSave: (collection) => call('collection_save', { collection }),
  collectionDelete: (id) => call('collection_delete', { id }),

  // request tabs
  requestTabsList: () => call('request_tabs_list'),
  requestTabSave: (tab) => call('request_tab_save', { tab }),
  requestTabDelete: (id) => call('request_tab_delete', { id }),

  // saved requests
  savedRequestsList: () => call('saved_requests_list'),
  savedRequestSave: (request) => call('saved_request_save', { request }),
  savedRequestDelete: (id) => call('saved_request_delete', { id }),

  // environments
  environmentsList: () => call('environments_list'),
  environmentSave: (environment) => call('environment_save', { environment }),
  environmentDelete: (id) => call('environment_delete', { id }),

  // http
  requestSend: (request, applyEnv = true, savedRequestId = null, name = null) =>
    call('request_send', { request, applyEnv, savedRequestId, name }),

  // app state
  stateGet: (key) => call('state_get', { key }),
  stateSet: (key, value) => call('state_set', { key, value }),

  // request globals (plain variables, applied under the active environment)
  globalsGet: () => call('state_get', { key: 'request_globals' }),
  globalsSet: (json) => call('state_set', { key: 'request_globals', value: json }),

  // MCP server
  mcpConfigGet: () => call('mcp_config_get'),
  mcpConfigSet: (config) => call('mcp_config_set', { config }),
  mcpStatus: () => call('mcp_status'),
  mcpStart: () => call('mcp_start'),
  mcpStop: () => call('mcp_stop'),
  mcpAclsGet: () => call('mcp_acls_get'),
  mcpAclSet: (connectionId, acl) => call('mcp_acl_set', { connectionId, acl })
};
