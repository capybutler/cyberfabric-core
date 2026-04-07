Created:  2026-03-20 by Constructor Tech
Updated:  2026-03-20 by Constructor Tech
# Feature: Message Search


<!-- toc -->

- [1. Feature Context](#1-feature-context)
  - [1.1 Overview](#11-overview)
  - [1.2 Purpose](#12-purpose)
  - [1.3 Actors](#13-actors)
  - [1.4 References](#14-references)
- [2. Actor Flows (CDSL)](#2-actor-flows-cdsl)
  - [Search Session Messages](#search-session-messages)
  - [Search Across Sessions](#search-across-sessions)
- [3. Processes / Business Logic (CDSL)](#3-processes--business-logic-cdsl)
  - [Parse Search Query](#parse-search-query)
  - [Paginate Results](#paginate-results)
  - [Load Context Window](#load-context-window)
  - [Maintain Search Index](#maintain-search-index)
- [4. States (CDSL)](#4-states-cdsl)
  - [None](#none)
- [5. Definitions of Done](#5-definitions-of-done)
  - [Session-Scoped Full-Text Search](#session-scoped-full-text-search)
  - [Cross-Session Full-Text Search](#cross-session-full-text-search)
  - [Search Result Pagination](#search-result-pagination)
  - [Search Index Maintenance](#search-index-maintenance)
- [6. Acceptance Criteria](#6-acceptance-criteria)
- [7. Non-Functional Considerations](#7-non-functional-considerations)

<!-- /toc -->

- [ ] `p3` - **ID**: `cpt-cf-chat-engine-featstatus-message-search`

## 1. Feature Context

- [ ] `p3` - `cpt-cf-chat-engine-feature-message-search`

**Traces to**: `cpt-cf-chat-engine-fr-search-session` (FR-012), `cpt-cf-chat-engine-fr-search-sessions` (FR-013), `cpt-cf-chat-engine-nfr-search` (NFR-010)

### 1.1 Overview

Full-text search across session message history using PostgreSQL tsvector with GIN indexes. Supports single-session search filtered by session_id and cross-session search scoped to the requesting client's tenant and user identity, with relevance-ranked and cursor-paginated result sets.

### 1.2 Purpose

Enable end-users to locate messages within a session's history or across all their sessions using full-text queries. Search results are ranked by relevance using ts_rank_cd with document length normalization and paginated using cursor-based queries for consistency.

Success criteria: Session-scoped search returns results within 1 second at p95 for sessions with up to 10,000 messages; cross-session search returns results within 3 seconds at p95 for clients with up to 1,000 sessions.

### 1.3 Actors

| Actor | Role in Feature |
|-------|-----------------|
| `cpt-cf-chat-engine-actor-client` | Submits search queries (session-scoped or cross-session), receives ranked paginated results |

### 1.4 References

- **PRD**: [PRD.md](../PRD.md)
- **Design**: [DESIGN.md](../DESIGN.md)
- **ADR**: [ADR-0019: PostgreSQL Full-Text Search with GIN Indexes](../ADR/0019-search-strategy.md)
- **Design Principles**: `cpt-cf-chat-engine-principle-immutable-tree` (message tree integrity), `cpt-cf-chat-engine-constraint-single-database` (PostgreSQL-only search, no external search engine)
- **Dependencies**: `cpt-cf-chat-engine-feature-message-processing`

## 2. Actor Flows (CDSL)

### Search Session Messages

- [ ] `p3` - **ID**: `cpt-cf-chat-engine-flow-message-search-search-session`

**Actor**: `cpt-cf-chat-engine-actor-client`

**Success Scenarios**:
- Client searches within a session and receives relevance-ranked results with surrounding message context

**Error Scenarios**:
- Session not found or not owned by caller (403/404)
- Query string is empty or exceeds maximum length (400)

**Steps**:
1. [ ] - `p3` - Algorithm: authenticate request using `cpt-cf-chat-engine-algo-session-lifecycle-authenticate` - `inst-ss-auth`
2. [ ] - `p3` - API: GET /sessions/{session_id}/search?q={query}&cursor={token}&per_page={n} - `inst-ss-api`
3. [ ] - `p3` - Algorithm: validate session ownership using `cpt-cf-chat-engine-algo-session-lifecycle-validate-ownership` - `inst-ss-ownership`
4. [ ] - `p3` - Algorithm: validate and parse search query using `cpt-cf-chat-engine-algo-message-search-parse-query` - `inst-ss-parse`
5. [ ] - `p3` - DB: Full-text search messages in session_id matching parsed query (excluding hidden messages), ranked by relevance then created_at descending - `inst-ss-search`
6. [ ] - `p3` - Algorithm: apply cursor-based pagination using `cpt-cf-chat-engine-algo-message-search-paginate` - `inst-ss-paginate`
7. [ ] - `p3` - Algorithm: load surrounding context messages using `cpt-cf-chat-engine-algo-message-search-load-context` - `inst-ss-context`
8. [ ] - `p3` - **RETURN** 200 (results[], total_count, next_cursor, per_page) - `inst-ss-return`

### Search Across Sessions

- [ ] `p3` - **ID**: `cpt-cf-chat-engine-flow-message-search-search-sessions`

**Actor**: `cpt-cf-chat-engine-actor-client`

**Success Scenarios**:
- Client searches across all owned sessions and receives relevance-ranked results grouped by session with session metadata

**Error Scenarios**:
- Query string is empty or exceeds maximum length (400)

**Steps**:
1. [ ] - `p3` - Algorithm: authenticate request using `cpt-cf-chat-engine-algo-session-lifecycle-authenticate` - `inst-xs-auth`
2. [ ] - `p3` - API: GET /sessions/search?q={query}&cursor={token}&per_page={n} - `inst-xs-api`
3. [ ] - `p3` - Algorithm: validate and parse search query using `cpt-cf-chat-engine-algo-message-search-parse-query` - `inst-xs-parse`
4. [ ] - `p3` - DB: Full-text search messages across all sessions owned by requesting user (tenant_id, user_id), excluding hard-deleted sessions and hidden messages, joined with session metadata, ranked by relevance then created_at descending - `inst-xs-search`
5. [ ] - `p3` - Algorithm: apply cursor-based pagination using `cpt-cf-chat-engine-algo-message-search-paginate` - `inst-xs-paginate`
6. [ ] - `p3` - **RETURN** 200 (results[], total_count, next_cursor, per_page) where each result includes session_id, session metadata, matched message, and rank - `inst-xs-return`

## 3. Processes / Business Logic (CDSL)

### Parse Search Query

- [ ] `p3` - **ID**: `cpt-cf-chat-engine-algo-message-search-parse-query`

**Input**: Raw query string from request parameter
**Output**: Parsed tsquery or 400 error

**Steps**:
1. [ ] - `p3` - **IF** query is empty or null **RETURN** 400 Bad Request (query required) - `inst-pq-check-empty`
2. [ ] - `p3` - **IF** query length exceeds configured maximum (default 500 characters) **RETURN** 400 Bad Request (query too long) - `inst-pq-check-length`
3. [ ] - `p3` - Sanitize query: strip characters that are invalid in tsquery syntax - `inst-pq-sanitize`
4. [ ] - `p3` - Convert sanitized query to a full-text search query using English language rules; use plain text conversion for simple queries or phrase-aware conversion for quoted phrases - `inst-pq-convert`
5. [ ] - `p3` - **RETURN** parsed tsquery value - `inst-pq-return`

### Paginate Results

- [ ] `p3` - **ID**: `cpt-cf-chat-engine-algo-message-search-paginate`

**Input**: Query result set, cursor (opaque token encoding last-seen rank + message_id, or null for first page), per_page limit
**Output**: Paginated result slice with next_cursor and total count

**Steps**:
1. [ ] - `p3` - **IF** per_page exceeds configured maximum (default 50) set per_page to maximum - `inst-pg-cap`
2. [ ] - `p3` - **IF** per_page < 1 set per_page to default (20) - `inst-pg-default`
3. [ ] - `p3` - **IF** cursor provided: decode cursor to (last_rank, last_message_id) and apply WHERE (rank, message_id) < (last_rank, last_message_id) keyset condition - `inst-pg-cursor`
4. [ ] - `p3` - Apply LIMIT (per_page + 1) to detect whether a next page exists - `inst-pg-apply`
5. [ ] - `p3` - **IF** result count > per_page: trim to per_page, encode next_cursor from last result's (rank, message_id) - `inst-pg-next`
6. [ ] - `p3` - Execute count query for total_count using the same filter criteria without pagination - `inst-pg-count`
7. [ ] - `p3` - **RETURN** (results[], total_count, next_cursor, per_page) - `inst-pg-return`

### Load Context Window

- [ ] `p3` - **ID**: `cpt-cf-chat-engine-algo-message-search-load-context`

**Input**: List of matched message_ids, session_id, context window size N (default 1 message before and after)
**Output**: Matched messages enriched with surrounding context messages

Context is loaded using two methods:
- **Primary (count-based)**: Retrieve N messages immediately before and N messages immediately after the matched message in chronological order within the same session.
- **Secondary (tree-position)**: If the matched message has a parent_message_id, include the parent chain up to the session root to provide thread context.

**Steps**:
1. [ ] - `p3` - **FOR EACH** matched message_id - `inst-lc-loop`
   1. [ ] - `p3` - DB: Retrieve N messages before and N messages after the matched message in the same session, ordered by created_at ascending, excluding hidden messages (count-based windowing) - `inst-lc-select`
   2. [ ] - `p3` - DB: **IF** matched message has parent_message_id: retrieve parent chain up to session root for thread context (tree-position windowing) - `inst-lc-parent-chain`
2. [ ] - `p3` - Assemble result: matched message with context_before[], context_after[], and parent_chain[] arrays - `inst-lc-assemble`
3. [ ] - `p3` - **RETURN** enriched results - `inst-lc-return`

### Maintain Search Index

- [ ] `p3` - **ID**: `cpt-cf-chat-engine-algo-message-search-maintain-index`

**Input**: Message create or delete event
**Output**: Updated search index

**Steps**:
1. [ ] - `p3` - **IF** event is message create: extract text content from message.content JSONB array, concatenate text parts - `inst-mi-extract`
2. [ ] - `p3` - **IF** event is message create: DB: Update the message record to populate content_tsvector with English full-text index of the extracted text - `inst-mi-update`
3. [ ] - `p3` - **IF** event is message delete: GIN index entries are automatically removed by PostgreSQL on row deletion - `inst-mi-delete`
4. [ ] - `p3` - **RETURN** void - `inst-mi-return`

## 4. States (CDSL)

### None

No state machines are required for this feature. Search is a stateless query operation with no entity lifecycle transitions.

## 5. Definitions of Done

### Session-Scoped Full-Text Search

- [ ] `p3` - **ID**: `cpt-cf-chat-engine-dod-message-search-session-search`

The system **MUST** accept GET /sessions/{session_id}/search with a query parameter, execute a full-text search against the messages table filtered by session_id using PostgreSQL tsvector with GIN indexes, return relevance-ranked results with surrounding context messages, and enforce session ownership via JWT identity.

**Implements**:
- `cpt-cf-chat-engine-flow-message-search-search-session`
- `cpt-cf-chat-engine-algo-message-search-parse-query`
- `cpt-cf-chat-engine-algo-message-search-load-context`

**Touches**:
- API: `GET /sessions/{session_id}/search`
- DB: `messages` (content_tsvector column, GIN index)
- Entities: `SearchResult`

### Cross-Session Full-Text Search

- [ ] `p3` - **ID**: `cpt-cf-chat-engine-dod-message-search-cross-session-search`

The system **MUST** accept GET /sessions/search with a query parameter, execute a full-text search across all sessions belonging to the requesting user (scoped by tenant_id and user_id from JWT), return relevance-ranked results with session metadata, and exclude hard-deleted sessions from results.

**Implements**:
- `cpt-cf-chat-engine-flow-message-search-search-sessions`
- `cpt-cf-chat-engine-algo-message-search-parse-query`
- `cpt-cf-chat-engine-algo-message-search-paginate`

**Touches**:
- API: `GET /sessions/search`
- DB: `messages` (content_tsvector column, GIN index), `sessions` (tenant_id, user_id scope)
- Entities: `SearchResult`

### Search Result Pagination

- [ ] `p3` - **ID**: `cpt-cf-chat-engine-dod-message-search-pagination`

The system **MUST** paginate search results using cursor-based pagination with configurable per_page (default 20, maximum 50) and return total_count and next_cursor for client-side navigation. Results are ordered by relevance rank descending, then by created_at descending as tiebreaker. Cursor encodes the keyset position (rank + message_id) for stable, consistent pagination.

**Implements**:
- `cpt-cf-chat-engine-algo-message-search-paginate`

**Touches**:
- API: `GET /sessions/{session_id}/search`, `GET /sessions/search` (cursor, per_page query parameters)
- DB: `messages` (keyset pagination queries)
- Entities: `SearchResult`

### Search Index Maintenance

- [ ] `p3` - **ID**: `cpt-cf-chat-engine-dod-message-search-index-maintenance`

The system **MUST** maintain a tsvector column on the messages table with a GIN index, populate the tsvector on message creation by extracting text from the JSONB content array, and rely on PostgreSQL cascade behavior for index cleanup on message deletion.

**Implements**:
- `cpt-cf-chat-engine-algo-message-search-maintain-index`

**Touches**:
- DB: `messages` (content_tsvector TSVECTOR column, GIN index on content_tsvector)
- Entities: `Message`

## 6. Acceptance Criteria

- [ ] Session-scoped search returns relevance-ranked results for a valid query within a session the caller owns
- [ ] Cross-session search returns results from all sessions belonging to the caller, excluding hard-deleted sessions
- [ ] Search queries are case-insensitive with English stemming (e.g., "running" matches "run")
- [ ] Phrase search is supported using quoted query terms
- [ ] Empty or null query returns 400 Bad Request
- [ ] Session-scoped search on a session not owned by the caller returns 403/404 consistent with ownership validation
- [ ] Search results include surrounding context messages (messages before and after the match)
- [ ] Cursor-based pagination parameters (cursor, per_page) control result windowing; per_page is capped at 50
- [ ] Session-scoped search returns results within 1 second at p95 for sessions with up to 10,000 messages
- [ ] Cross-session search returns results within 3 seconds at p95 for clients with up to 1,000 sessions
- [ ] Messages with is_hidden_from_user=true are excluded from search results

## 7. Non-Functional Considerations

- **Performance**: Session-scoped search < 1s p95 (10K messages). Cross-session search < 3s p95 (1K sessions). GIN index enables sub-linear query scaling. Cross-session queries partitioned by tenant_id and user_id to prevent noisy neighbors.
- **Security**: Search queries scoped by JWT identity (tenant_id, user_id). Session ownership validated before session-scoped search. Hidden messages (is_hidden_from_user=true) never appear in results. Query input sanitized to prevent tsquery injection.
- **Reliability**: Search index consistency is maintained via synchronous indexing on message write. Index rebuild can be triggered via admin operation if needed.
- **Data**: GIN index on content_tsvector column. Composite index on (session_id, content_tsvector) for session-scoped queries. Index storage overhead approximately 20% of content column size. Index updates add approximately 5ms write latency per message.
- **Observability**: Metrics: `search_duration_seconds` (labeled by scope: session, cross_session). Log events for search queries with `trace_id`, `session_id` (if scoped), `query_length`, `result_count`, `duration_ms`.
- **Compliance / UX / Business**: Not applicable -- see session-lifecycle section 7.
