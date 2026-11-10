use crate::{events::EventHub,model::{Command,Status,TaskSpec}};
use axum::{Router,Json,extract::{State,Query,DefaultBodyLimit},http::{HeaderMap,StatusCode,Request},middleware::{self,Next},response::{Response,sse::{Event,Sse,KeepAlive}},routing::{get,post}};
use std::{sync::{Arc,Mutex,mpsc::{SyncSender,TrySendError}},convert::Infallible};
use serde::Deserialize;
use serde_json::{Value,json};
use tokio_stream::{StreamExt,wrappers::BroadcastStream};
#[derive(Clone)]
pub struct ApiState{pub status:Arc<Mutex<Status>>,pub commands:SyncSender<Command>,pub events:Arc<EventHub>,pub token:String}
async fn auth(State(s):State<ApiState>,request:Request<axum::body::Body>,next:Next)->Response{
 let expected=format!("Bearer {}",s.token);let received=request.headers().get("authorization").and_then(|v|v.to_str().ok()).unwrap_or("");
 let valid=!s.token.is_empty()&&received.len()==expected.len()&&received.bytes().zip(expected.bytes()).fold(0,|a,(b,c)|a|(b^c))==0;
 if !valid{return (StatusCode::UNAUTHORIZED,Json(json!({"error":"unauthorized"}))).into_response()}
 next.run(request).await
}
use axum::response::IntoResponse;
pub fn router(s:ApiState)->Router{Router::new().route("/status",get(status)).route("/ws",get(websocket)).route("/world/objects",get(objects)).route("/tasks",post(start)).route("/task/pause",post(pause)).route("/task/resume",post(resume)).route("/task/cancel",post(cancel)).route("/shutdown",post(shutdown)).route("/events",get(events)).route("/events/history",get(history)).layer(DefaultBodyLimit::max(32*1024)).layer(middleware::from_fn_with_state(s.clone(),auth)).with_state(s)}
pub async fn serve(listener:tokio::net::TcpListener,state:ApiState)->anyhow::Result<()>{if !listener.local_addr()?.ip().is_loopback(){anyhow::bail!("API must bind loopback")};axum::serve(listener,router(state)).await?;Ok(())}
async fn status(State(s):State<ApiState>)->Result<Json<Status>,StatusCode>{Ok(Json(s.status.lock().map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?.clone()))}
fn enqueue(s:ApiState,c:Command)->(StatusCode,Json<Value>){match s.commands.try_send(c){Ok(())=>(StatusCode::ACCEPTED,Json(json!({"accepted":true,"note":"queued, not yet executed"}))),Err(TrySendError::Full(_))=>(StatusCode::TOO_MANY_REQUESTS,Json(json!({"error":"command_queue_full"}))),Err(TrySendError::Disconnected(_))=>(StatusCode::SERVICE_UNAVAILABLE,Json(json!({"error":"agent_unavailable"})))}}
async fn start(State(s):State<ApiState>,body:Result<Json<TaskSpec>,axum::extract::rejection::JsonRejection>)->(StatusCode,Json<Value>){match body{Ok(Json(task))=>enqueue(s,Command::Start(task)),Err(_)=>(StatusCode::BAD_REQUEST,Json(json!({"error":"invalid_task_json"})))}}
async fn pause(State(s):State<ApiState>)->(StatusCode,Json<Value>){enqueue(s,Command::Pause)}
async fn resume(State(s):State<ApiState>)->(StatusCode,Json<Value>){enqueue(s,Command::Resume)}
async fn cancel(State(s):State<ApiState>)->(StatusCode,Json<Value>){enqueue(s,Command::Cancel)}
async fn shutdown(State(s):State<ApiState>)->(StatusCode,Json<Value>){enqueue(s,Command::Shutdown)}
#[derive(Deserialize)]struct Cursor{#[serde(default)]after:u64}
async fn history(State(s):State<ApiState>,Query(q):Query<Cursor>)->Result<Json<crate::events::History>,StatusCode>{s.events.history(q.after,256).map(Json).map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)}
async fn events(State(s):State<ApiState>,headers:HeaderMap)->Result<Sse<impl tokio_stream::Stream<Item=Result<Event,Infallible>>>,StatusCode>{
 let receiver=s.events.subscribe();
 let after=match headers.get("last-event-id"){Some(v)=>v.to_str().ok().and_then(|s|s.parse::<u64>().ok()).ok_or(StatusCode::BAD_REQUEST)?,None=>0};
 let replay=s.events.history(after,256).map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
 let mut initial=Vec::new();if replay.gap{initial.push(Ok(Event::default().event("gap").data(json!({"oldest":replay.oldest,"latest":replay.latest}).to_string())))}
 let mut last=after;
 for event in replay.events{last=event.id;initial.push(Ok(Event::default().event("agent").id(event.id.to_string()).data(serde_json::to_string(&event).unwrap())))}
 // Replay is paginated; clients resume from last ID via history when too far behind.
 if last<replay.latest{initial.push(Ok(Event::default().event("gap").data(json!({"after":last,"latest":replay.latest,"recover":"/events/history"}).to_string())));}
 // Drop queued replay duplicates. Stream lag is explicit rather than silently losing events.
 let live=BroadcastStream::new(receiver).filter_map(move|item|match item{Ok(event)if event.id>last=>{last=event.id;Some(Ok(Event::default().event("agent").id(event.id.to_string()).data(serde_json::to_string(&event).unwrap())))},Ok(_)=>None,Err(_)=>Some(Ok(Event::default().event("gap").data("{\"reason\":\"subscriber_lagged\",\"recover\":\"/events/history\"}")))});
 Ok(Sse::new(tokio_stream::iter(initial).chain(live)).keep_alive(KeepAlive::default()))
}

#[derive(Deserialize)]struct Page{#[serde(default)]offset:usize,#[serde(default="page_limit")]limit:usize}
fn page_limit()->usize{100}
async fn objects(State(s):State<ApiState>,Query(q):Query<Page>)->Result<Json<Value>,StatusCode>{
 if q.limit==0||q.limit>200{return Err(StatusCode::BAD_REQUEST)}
 let current=s.status.lock().map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
 let available=current.objects.len();let end=q.offset.saturating_add(q.limit).min(available);
 let rows=if q.offset>=available{&[][..]}else{&current.objects[q.offset..end]};
 Ok(Json(json!({"count":current.visible_objects,"available":available,"partial":available<current.visible_objects,"offset":q.offset,"next":if end<available{Some(end)}else{None},"objects":rows})))
}
async fn websocket(State(s):State<ApiState>,Query(q):Query<Cursor>,upgrade:axum::extract::ws::WebSocketUpgrade)->Response{
 upgrade.max_message_size(16*1024).max_frame_size(16*1024).on_upgrade(move|socket|ws_loop(socket,s,q.after))
}
async fn ws_loop(mut socket:axum::extract::ws::WebSocket,s:ApiState,after:u64){
 use axum::extract::ws::Message;
 let mut receiver=s.events.subscribe();
 let snapshot=match s.status.lock(){Ok(status)=>status.clone(),Err(_)=>return};
 if socket.send(Message::Text(json!({"kind":"state.snapshot","state":snapshot}).to_string().into())).await.is_err(){return}
 let replay=match s.events.history(after,256){Ok(h)=>h,Err(_)=>return};let mut last=after;
 if replay.gap&&socket.send(Message::Text(json!({"kind":"gap","oldest":replay.oldest,"latest":replay.latest,"recover":"/events/history"}).to_string().into())).await.is_err(){return}
 for event in replay.events{last=event.id;if socket.send(Message::Text(json!({"kind":"event","event":event}).to_string().into())).await.is_err(){return}}
 if last<replay.latest&&socket.send(Message::Text(json!({"kind":"gap","after":last,"latest":replay.latest,"recover":"/events/history"}).to_string().into())).await.is_err(){return}
 loop{tokio::select!{
 incoming=socket.recv()=>match incoming{None|Some(Err(_))|Some(Ok(Message::Close(_)))=>break,Some(Ok(Message::Ping(p)))=>{if socket.send(Message::Pong(p)).await.is_err(){break}},Some(Ok(Message::Text(_)|Message::Binary(_)))=>{if socket.send(Message::Text(json!({"kind":"error","reason":"read_only_event_stream_use_http_for_tasks"}).to_string().into())).await.is_err(){break}},_=>{}},
 event=receiver.recv()=>match event{Ok(event)if event.id>last=>{last=event.id;if socket.send(Message::Text(json!({"kind":"event","event":event}).to_string().into())).await.is_err(){break}},Ok(_)=>{},Err(tokio::sync::broadcast::error::RecvError::Lagged(_))=>{if socket.send(Message::Text(json!({"kind":"gap","after":last,"recover":"/events/history"}).to_string().into())).await.is_err(){break}},Err(_)=>break}
 }}
}
