use std::net::SocketAddr;

use super::Transport;
use crate::config::{TlsConfig, TransportConfig};
use crate::helper::set_tcp_keepalive;
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use tokio::fs;
use tokio::net::{TcpListener, TcpStream, ToSocketAddrs};
use tokio_native_tls::native_tls::{self, Certificate, Identity};
use tokio_native_tls::{TlsAcceptor, TlsConnector, TlsStream};

#[derive(Debug)]
pub struct TlsTransport {
    config: TlsConfig,
    connector: Option<TlsConnector>,
}

#[async_trait]
impl Transport for TlsTransport {
    type Acceptor = (TcpListener, TlsAcceptor);
    type Stream = TlsStream<TcpStream>;

    async fn new(config: &TransportConfig) -> Result<Self> {
        let config = match &config.tls {
            Some(v) => v,
            None => {
                return Err(anyhow!("Missing tls config"));
            }
        };

        let connector = match config.trusted_root.as_ref() {
            Some(path) => {
                let s = fs::read_to_string(path)
                    .await
                    .with_context(|| "Failed to read the `tls.trusted_root`")?;
                let cert = Certificate::from_pem(s.as_bytes())
                    .with_context(|| "Failed to read certificate from `tls.trusted_root`")?;
                let connector = native_tls::TlsConnector::builder()
                    .add_root_certificate(cert)
                    .build()?;
                Some(TlsConnector::from(connector))
            }
            None => None,
        };

        Ok(TlsTransport {
            config: config.clone(),
            connector,
        })
    }

    async fn bind<A: ToSocketAddrs + Send + Sync>(&self, addr: A) -> Result<Self::Acceptor> {
        let ident = Identity::from_pkcs12(
            &fs::read(self.config.pkcs12.as_ref().unwrap()).await?,
            self.config.pkcs12_password.as_ref().unwrap(),
        )
        .with_context(|| "Failed to create identitiy")?;
        let l = TcpListener::bind(addr)
            .await
            .with_context(|| "Failed to create tcp listener")?;
        let t = TlsAcceptor::from(native_tls::TlsAcceptor::new(ident).unwrap());
        Ok((l, t))
    }

    async fn accept(&self, a: &Self::Acceptor) -> Result<(Self::Stream, SocketAddr)> {
        let (conn, addr) = a.0.accept().await?;
        set_tcp_keepalive(&conn);

        let conn = a.1.accept(conn).await?;

        Ok((conn, addr))
    }

    async fn connect(&self, addr: &str) -> Result<Self::Stream> {
        let conn = TcpStream::connect(&addr).await?;
        set_tcp_keepalive(&conn);

        let connector = self.connector.as_ref().unwrap();
        Ok(connector
            .connect(
                self.config
                    .hostname
                    .as_ref()
                    .unwrap_or(&String::from(addr.split(':').next().unwrap())),
                conn,
            )
            .await?)
    }
}
