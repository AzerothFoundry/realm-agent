use realm_agent::{api::{router,ApiState},events::EventHub,model::{Status,Command}};
use std::{sync::{Arc,Mutex,mpsc},path::PathBuf};
use axum::{body::Body,http::{Request,StatusCode}};
use tower::ServiceExt;
use serde_json::json;
fn path()->PathBuf{use std::sync::atomic::{AtomicU64,Ordering};static N:AtomicU64=AtomicU64::new(0);std::env::temp_dir().join(format!("realm-agent-events-{}-{}.jsonl",std::process::id(),N.fetch_add(1,Ordering::Relaxed)))}
fn setup()->(ApiState,mpsc::Receiver<Command>,PathBuf){let p=path();let(tx,rx)=mpsc::sync_channel(1);(ApiState{status:Arc::new(Mutex::new(Status::default())),commands:tx,events:Arc::new(EventHub::new(p.clone(),4).unwrap()),token:"test-local-token".into()},rx,p)}
#[tokio::test]async fn authentication_and_queue_backpressure(){let(s,rx,p)=setup();let app=router(s);
 assert_eq!(app.clone().oneshot(Request::builder().uri("/status").body(Body::empty()).unwrap()).await.unwrap().status(),StatusCode::UNAUTHORIZED);
 let req=||Request::builder().method("POST").uri("/task/pause").header("Authorization","Bearer test-local-token").body(Body::empty()).unwrap();
 assert_eq!(app.clone().oneshot(req()).await.unwrap().status(),StatusCode::ACCEPTED);
 assert_eq!(app.clone().oneshot(req()).await.unwrap().status(),StatusCode::TOO_MANY_REQUESTS);assert!(matches!(rx.recv().unwrap(),Command::Pause));
 let bad=Request::builder().method("POST").uri("/tasks").header("Authorization","Bearer test-local-token").header("content-type","application/json").body(Body::from("{\"unknown\":true}")).unwrap();assert_eq!(app.oneshot(bad).await.unwrap().status(),StatusCode::BAD_REQUEST);std::fs::remove_file(p).unwrap();}
#[tokio::test]async fn status_history_and_sse_auth(){let(s,_rx,p)=setup();for n in 0..6{s.events.emit(None,"tick",json!({"n":n})).unwrap();}let h=s.events.history(1,256).unwrap();assert!(h.gap);assert_eq!(h.oldest,3);assert_eq!(h.events.len(),4);
 let app=router(s);let req=Request::builder().uri("/events").body(Body::empty()).unwrap();assert_eq!(app.clone().oneshot(req).await.unwrap().status(),StatusCode::UNAUTHORIZED);
 let req=Request::builder().uri("/events").header("authorization","Bearer test-local-token").header("last-event-id","oops").body(Body::empty()).unwrap();assert_eq!(app.clone().oneshot(req).await.unwrap().status(),StatusCode::BAD_REQUEST);
 let req=Request::builder().uri("/status").header("authorization","Bearer test-local-token").body(Body::empty()).unwrap();let response=app.oneshot(req).await.unwrap();assert_eq!(response.status(),StatusCode::OK);std::fs::remove_file(p).unwrap();}
#[test]fn local_file_permissions_no_overwrite_and_secret_rejection(){let p=path();let hub=EventHub::new(p.clone(),4).unwrap();assert!(EventHub::new(p.clone(),4).is_err());assert!(hub.emit(None,"bad",json!({"password":"never-log"})).is_err());let first=hub.emit(Some(1),"task_started",json!({"phase":"prepare"})).unwrap();assert_eq!(first.id,1);
 #[cfg(unix)]{use std::os::unix::fs::PermissionsExt;assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode()&0o777,0o600);}
 drop(hub);let text=std::fs::read_to_string(&p).unwrap();assert!(text.contains("task_started"));assert!(!text.contains("never-log"));std::fs::remove_file(p).unwrap();}
#[test]fn bounded_rotation(){let p=path();let hub=EventHub::new(p.clone(),4).unwrap();let detail=json!({"payload":"x".repeat(60000)});for _ in 0..100{hub.emit(None,"sample",detail.clone()).unwrap();}assert!(p.with_extension("jsonl.1").exists());assert!(std::fs::metadata(&p).unwrap().len()<=5*1024*1024);drop(hub);std::fs::remove_file(&p).unwrap();for i in 1..=5{let _=std::fs::remove_file(p.with_extension(format!("jsonl.{i}")));}}

#[tokio::test]async fn websocket_real_tcp_snapshot_replay_and_live(){
 use futures_util::{StreamExt as _,SinkExt as _};use tokio_tungstenite::tungstenite::{client::IntoClientRequest,Message};
 let(s,_rx,p)=setup();s.events.emit(None,"before",json!({})).unwrap();let hub=s.events.clone();
 let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();let address=listener.local_addr().unwrap();let server=tokio::spawn(realm_agent::api::serve(listener,s));
 assert!(tokio_tungstenite::connect_async(format!("ws://{address}/ws")).await.is_err());
 let mut request=format!("ws://{address}/ws?after=0").into_client_request().unwrap();request.headers_mut().insert("authorization","Bearer test-local-token".parse().unwrap());
 let(mut socket,response)=tokio_tungstenite::connect_async(request).await.unwrap();assert_eq!(response.status(),101);
 let read=|m:Message|serde_json::from_str::<serde_json::Value>(m.to_text().unwrap()).unwrap();
 assert_eq!(read(socket.next().await.unwrap().unwrap())["kind"],"state.snapshot");
 assert_eq!(read(socket.next().await.unwrap().unwrap())["event"]["kind"],"before");
 hub.emit(None,"after",json!({})).unwrap();assert_eq!(read(socket.next().await.unwrap().unwrap())["event"]["kind"],"after");
 socket.send(Message::Text("raw_cast".into())).await.unwrap();assert_eq!(read(socket.next().await.unwrap().unwrap())["kind"],"error");socket.close(None).await.unwrap();server.abort();std::fs::remove_file(p).unwrap();
}
#[tokio::test]async fn world_objects_pagination_is_authenticated_and_bounded(){
 let(s,_rx,p)=setup();s.status.lock().unwrap().visible_objects=600;
 let app=router(s);let request=|uri:&str|Request::builder().uri(uri).header("authorization","Bearer test-local-token").body(Body::empty()).unwrap();
 assert_eq!(app.clone().oneshot(request("/world/objects?limit=201")).await.unwrap().status(),StatusCode::BAD_REQUEST);
 let response=app.oneshot(request("/world/objects?offset=0&limit=100")).await.unwrap();assert_eq!(response.status(),StatusCode::OK);
 use http_body_util::BodyExt;let bytes=response.into_body().collect().await.unwrap().to_bytes();let result:serde_json::Value=serde_json::from_slice(&bytes).unwrap();assert_eq!(result["count"],600);assert_eq!(result["partial"],true);std::fs::remove_file(p).unwrap();
}
