//! The separate UDP service observed in the native Mac capture.
use owly_airdrop::proto::mdns::{self, Message, Name, RData, Record, Service, rtype};

pub struct QuicService {
    records: Vec<Record>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdns::Question;

    #[test]
    fn quic_discovery_publishes_udp_endpoint_and_withdraws_it() {
        let service = Service {
            instance: "001122334455".into(),
            host: "airbridge".into(),
            port: 8770,
            addr: "fe80::1".parse().unwrap(),
            flags: 138,
        };
        let quic = QuicService::new(&service, "00000000-0000-4000-8000-001122334455");
        let question = Question {
            name: Name::new(&["_asquic", "_udp", "local"]),
            qtype: rtype::PTR,
            unicast: false,
        };
        let query = Message {
            id: 23,
            questions: vec![question],
            ..Message::default()
        };
        let reply = quic.answer(&query, false).unwrap();
        assert!(!reply.unicast);
        let decoded = Message::decode(&reply.message).unwrap();
        assert_eq!(decoded.id, 0);
        assert!(decoded.answers.iter().any(|r| matches!(&r.data,
            RData::Srv { port: 8770, target, .. } if *target == Name::host("airbridge"))));
        assert!(
            decoded
                .answers
                .iter()
                .any(|r| r.data == RData::Aaaa(service.addr))
        );
        assert!(
            decoded
                .answers
                .iter()
                .any(|r| matches!(&r.data, RData::Txt(txt) if txt.is_empty()))
        );
        let legacy = quic.answer(&query, true).unwrap();
        assert!(legacy.unicast);
        let decoded = Message::decode(&legacy.message).unwrap();
        assert_eq!(decoded.id, 23);
        assert!(
            decoded
                .answers
                .iter()
                .all(|r| r.ttl <= 10 && !r.cache_flush)
        );
        assert!(
            quic.answer(&Message::decode(&quic.announcement(false)).unwrap(), false)
                .is_none()
        );
        assert!(
            Message::decode(&quic.announcement(true))
                .unwrap()
                .answers
                .iter()
                .all(|r| r.ttl == 0)
        );
    }
}

impl QuicService {
    pub fn new(service: &Service, instance: &str) -> Self {
        let name = Name::new(&[instance, "_asquic", "_udp", "local"]);
        let mut records = service.records();
        records[0].name = Name::new(&["_asquic", "_udp", "local"]);
        records[0].data = RData::Ptr(name.clone());
        records[1].name = name.clone();
        records[2].name = name;
        records[2].data = RData::Txt(Vec::new());
        Self { records }
    }

    pub fn announcement(&self, goodbye: bool) -> Vec<u8> {
        let mut answers = self.records.clone();
        if goodbye {
            for record in &mut answers {
                record.ttl = 0;
            }
        }
        Message {
            response: true,
            answers,
            ..Message::default()
        }
        .encode()
    }

    pub fn answer(&self, query: &Message, legacy: bool) -> Option<mdns::Reply> {
        if query.response
            || !query.questions.iter().any(|question| {
                self.records.iter().any(|record| {
                    record.name == question.name
                        && (question.qtype == record.rtype || question.qtype == rtype::ANY)
                })
            })
        {
            return None;
        }
        let mut answers = self.records.clone();
        if legacy {
            for record in &mut answers {
                record.cache_flush = false;
                record.ttl = record.ttl.min(10);
            }
        }
        Some(mdns::Reply {
            unicast: legacy || query.questions.iter().all(|q| q.unicast),
            message: Message {
                id: if legacy { query.id } else { 0 },
                response: true,
                questions: if legacy {
                    query.questions.clone()
                } else {
                    Vec::new()
                },
                answers,
                ..Message::default()
            }
            .encode(),
        })
    }
}
