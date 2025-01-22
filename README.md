[![dependency status](https://deps.rs/repo/github/silicon-heaven/shv-http-gateway/status.svg)](https://deps.rs/repo/github/silicon-heaven/shv-http-gateway)

# Config options

 - `--broker-url`: URL to the broker to establish connections upon login requests (e.g.: `tcp://localhost:3755`)
 - `--max-user-sessions`: Maximum number of opened sessions and subscriptions per a user (default: 10)
 - `--session-timeout`: A session time-outs when no request is sent within the timeout interval and there is not any opened subscriptions event stream (10 mins)
 - `--heartbeat-interval`: Heartbeat interval of connections to the broker (default: 60 s)

# API Documentation

## Login

This endpoint is used to authenticate a user by providing their username and password. Upon successful authentication, a session ID will be returned, which can be used for further requests that require authentication.

### Request

#### URL
`POST /api/login`

#### Request Body (JSON)
```json
{
  "username": "john",
  "password": "secret123"
}
```

- **username** (string): The username of the user trying to authenticate.
- **password** (string): The password associated with the username.

### Responses

#### Success

- **Status**: `200 OK`
- **Response Body** (JSON):
  ```json
  {
    "session_id": "heASkr1MBntPPg7s0BsjTP7Ibyedb5EYlnzKaQH1"
  }
  ```
  - **session_id** (string): A unique session ID returned upon successful authentication. This session ID must be included in subsequent requests that require authentication.

#### Error Responses

- **Status**: `422 Unprocessable Entity`
  - **Description**: The request body is malformed or missing required fields.

- **Status**: `500 Internal Server Error`
  - **Description**: There is an issue with the broker configuration.

- **Status**: `503 Service Unavailable`
  - **Description**: The API is unable to connect to the broker.

- **Status**: `401 Unauthorized`
  - **Description**: The provided credentials are incorrect.

### Example Request

```bash
curl -X POST https://yourapi.com/api/login \
  -H "Content-Type: application/json" \
  -d '{"username": "john", "password": "secret123"}'
```

### Example Success Response

```json
{
  "session_id": "heASkr1MBntPPg7s0BsjTP7Ibyedb5EYlnzKaQH1"
}
```

### Example Error Response

```json
{
  "code": 401,
  "detail": "Invalid credentials."
}
```

---

## Logout

This endpoint is used to log out the authenticated user by invalidating their session token. The request must include the `Authorization` header containing the valid session ID.

### Request

#### URL
`POST /api/logout`

#### Headers
- **Authorization** (string): The session token that was provided during login. The value should be the session ID received from the `/api/login` endpoint.

#### Request Body
- The request body is empty.

### Responses

#### Success

- **Status**: `200 OK`
- **Response Body**: Empty

#### Error Responses

- **Status**: `400 Bad Request`
  - **Description**: The `Authorization` header is missing.

- **Status**: `401 Unauthorized`
  - **Description**: The provided session token is invalid or has expired.

### Example Request

```bash
curl -X POST https://yourapi.com/api/logout \
  -H "Authorization: heASkr1MBntPPg7s0BsjTP7Ibyedb5EYlnzKaQH1"
```

### Example Error Responses

```json
{
  "code": 400,
  "detail": "Missing Authorization header"
}
```

```json
{
  "code": 401,
  "detail": "Invalid session token"
}
```

---

## Call RPC method

This endpoint is used to call RPC methods.

### Request

#### URL
`POST /api/rpc`

#### Headers
- **Authorization** (string): The session token that was provided during login. The value should be the session ID received from the `/api/login` endpoint.

#### Request Body (JSON)
```json
{
    "path": "shv/foo/bar",
    "method": "getFile",
    "param": "{\"maxSize\":1234}"
}
```

- **path** (string): SHV path
- **method** (string): SHV method to call on the path
- **param** (string, optional): Parameter to the call in CPON format

### Responses

#### Success

- **Status**: `200 OK`
- **Response Body** (JSON):
  ```json
  {
      "result": "42"
  }
  ```
  - **result** (string): Result as a CPON string

#### Error
- **Error Response Format:**
  ```json
  {
    "code": <HTTP_STATUS_CODE>,
    "detail": "<ERROR_MESSAGE>",
    "shv_error": "<SHV_ERROR_KIND>" // Only present when `code` is 500
  }
  ```

- **Possible Errors:**
- **Status**: `400 Bad Request`
  - **Description**: The `Authorization` header is missing.

- **Status**: `401 Unauthorized`
  - **Description**: The provided session token is invalid or has expired.

- **Status**: `422 Unprocessable entity`
  - **Description**: The request body is malformed or missing required fields..

- **Status Code:** `500 Internal Server Error`
  - **Description**:
      An error occurred during the method call in the SHV stack. In this case the `shv_error` field is present and can have one of the following values:
      - `ConnectionClosed`
      - `InvalidMessage`
      - `ResultTypeMismatch`
      - `RpcError(<RPC_ERROR_KIND>)`, where `<RPC_ERROR_KIND>` can be:
        - `NoError`
        - `InvalidRequest`
        - `MethodNotFound`
        - `InvalidParam`
        - `InternalError`
        - `ParseError`
        - `MethodCallTimeout`
        - `MethodCallCancelled`
        - `MethodCallException`
        - `PermissionDenied`
        - `Unknown`
        - `UserCode`

### Example Request
```bash
curl -X POST https://example.com/api/rpc \
  -H "Content-Type: application/json" \
  -H "Authorization: heASkr1MBntPPg7s0BsjTP7Ibyedb5EYlnzKaQH1" \
  -d '{
    "path": "foo/bar/xyz",
    "method": "getFile",
    "param": "{\"maxSize:1234\"}"
  }'
```

### Example Success Response
```json
{
  "result": "42"
}
```

### Example Error Responses

**400 Bad Request**
```json
{
  "code": 400,
  "detail": "Missing Authorization header"
}
```

**401 Invalid session token**
```json
{
  "code": 401,
  "detail": "Invalid session token"
}
```

**500 Internal Server Error with SHV Error**
```json
{
  "code": 500,
  "detail": "RPC method on path `foo/bar` not found.",
  "shv_error": "RpcError(MethodNotFound)"
}
```

---

## Subscribe to notifications

Subscribe to a notification stream for specific signals. The server sends events as an HTTP event stream.

### Request

#### URL
`POST /api/subscribe`

#### Headers
- **Authorization** (string): The session token that was provided during login. The value should be the session ID received from the `/api/login` endpoint.

#### Request Body (JSON)
```json
{
    "path": "shv/foo/bar",
    "signal": "chng"
}
```

- **path** (string): SHV path of the resource
- **signal** (string): Signal name to listen to

### Responses

#### Success
- **Status Code:** `200 OK`
- **Response Type:** `text/event-stream`
- **Event Stream Data Format:**
  - **Notification Event:**
    ```
    data: {"path": "path/of/the/notification", "signal": "signal_name", "param": "..."}
    ```
    - `path`: Path of the notification.
    - `signal`: Signal name associated with the notification.
    - `param`: CPON string containing additional data for the notification.

- If an error frame is received, the server sends an error event in the stream:
  ```
  event: error
  data: <ERROR_MESSAGE>
  ```

#### Error
- **Error Response Format:**
  ```json
  {
    "code": <HTTP_STATUS_CODE>,
    "detail": "<ERROR_MESSAGE>",
  }
  ```

- **Possible Errors:**
- **Status**: `400 Bad Request`
  - **Description**: The `Authorization` header is missing.

- **Status**: `401 Unauthorized`
  - **Description**: The provided session token is invalid or has expired.

- **Status**: `422 Unprocessable entity`
  - **Description**: The request body is malformed or missing required fields..

- **Status Code:** `500 Internal Server Error`
  - **Description**: Cannot make the subscription.

### Example Request
```bash
curl -X POST https://example.com/api/subscribe \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <session_id>" \
  -d '{
    "path": "foo/bar",
    "signal": "signal_name"
  }'
```

### Example Successful Event Stream
**Server Response (HTTP 200):**
```
...

data: {"path": "foo/bar/notification", "signal": "signal_name", "param": "..."}

data: {"path": "foo/bar/another_notification", "signal": "another_signal", "param": "..."}

...
```

### Notes
- The client must maintain an active connection to receive events.
- The event stream will terminate if the server encounters an unrecoverable error or the client disconnects.

