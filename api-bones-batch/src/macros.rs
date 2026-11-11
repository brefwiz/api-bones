// SPDX-License-Identifier: MIT
//! `batch_method!`: the per-method glue between a generated Connect client and
//! the batch runtime.
//!
//! Chunk sizing counts a one-byte tag per element (`chunk::element_bytes`).
//! The ids field of the batch request may carry a larger tag, so the template
//! size handed to the planner reserves the extra tag bytes of a full chunk.

use crate::chunk::MAX_ITEMS;

fn varint_len(mut value: u64) -> usize {
    let mut len = 1;
    while value >= 0x80 {
        value >>= 7;
        len += 1;
    }
    len
}

/// Encoded length of the tag of a length-delimited field with this number.
#[must_use]
pub fn tag_len(field_number: u32) -> usize {
    varint_len((u64::from(field_number) << 3) | 2)
}

/// Template size for the planner: the encoded template plus, for a full
/// chunk of [`MAX_ITEMS`] ids, the tag bytes beyond the one the planner counts.
#[must_use]
pub fn template_bytes(encoded_len: u32, ids_field: u32) -> usize {
    let template = usize::try_from(encoded_len).unwrap_or(usize::MAX);
    template.saturating_add(MAX_ITEMS.saturating_mul(tag_len(ids_field).saturating_sub(1)))
}

/// Internal helper of [`batch_method!`]; not part of the public API.
#[doc(hidden)]
#[macro_export]
macro_rules! __batch_item_error {
    ($error:expr) => {{
        let error = &$error;
        $crate::ItemErrorParts {
            code: error.code.clone(),
            message: error.message.clone(),
            info: error
                .info
                .as_option()
                .map(|info| (info.code.clone(), info.emitter.clone())),
            violations: error
                .violations
                .iter()
                .map(|violation| {
                    (
                        violation.field.clone(),
                        violation.message.clone(),
                        violation.rule.clone(),
                    )
                })
                .collect(),
        }
    }};
}

/// Declares the batch glue of one RPC pair on [`Batched`](crate::Batched), as
/// the extension trait `ext`.
///
/// Get form: `get` names the single Get, `batch` its `BatchGet`. The extension
/// trait gets the coalesced `name` / `with_options` of the Get and the
/// explicit `name` / `with_options` of the batch. The consumer provides
/// `From<&GetRequest> for BatchTemplate`.
///
/// List form: `list` names the single List, `batch` its `BatchList`. The trait
/// gets the grouped batch methods. The consumer provides
/// `ListRequestFor<BatchTemplate>` for the List request, and
/// `PagedRequest` / `PagedResponse` for the List request and response.
///
/// `ids_field` / `parents_field` is the proto field number of the ids field of
/// the batch request; it determines the tag length when chunks are sized. A missing field or a wrong type is
/// a compile error.
///
/// ```ignore
/// batch_method! {
///     ext: FooBatchExt,
///     client: FooServiceClient<HttpTransport>,
///     get: {
///         method: "pkg.FooService/GetFoo",
///         name: get_foo,
///         with_options: get_foo_with_options,
///         request: GetFooRequest,
///         id: id,
///         item: Foo,
///     },
///     batch: {
///         method: "pkg.FooService/BatchGetFoos",
///         name: batch_get_foos,
///         with_options: batch_get_foos_with_options,
///         template: BatchGetFoosRequest,
///         ids: ids,
///         ids_field: 1,
///         results: results,
///         outcome: foo_result::Outcome,
///     },
/// }
/// ```
#[macro_export]
macro_rules! batch_method {
    (
        ext: $ext:ident,
        client: $client:ty,
        get: {
            method: $single_method:literal,
            name: $get:ident,
            with_options: $get_opts:ident,
            request: $get_req:ty,
            id: $id:ident,
            item: $item:ty $(,)?
        },
        batch: {
            method: $batch_method:literal,
            name: $batch:ident,
            with_options: $batch_opts:ident,
            template: $template:ty,
            ids: $ids:ident,
            ids_field: $ids_field:literal,
            results: $results:ident,
            outcome: $outcome:path $(,)?
        } $(,)?
    ) => {
        /// Coalesced single gets and explicit batch gets of one method pair.
        pub trait $ext {
            /// A single get, merged with the gets of the same tick that share
            /// its identity, headers and non-id fields into one batch request.
            fn $get(
                &self,
                request: $get_req,
            ) -> impl ::core::future::Future<Output = $crate::ItemResult<$item>> + Send;

            /// The same call with per-call headers and deadline.
            fn $get_opts(
                &self,
                request: $get_req,
                headers: $crate::CallHeaders,
            ) -> impl ::core::future::Future<Output = $crate::ItemResult<$item>> + Send;

            /// An explicit batch: any number of ids, one outcome per id in
            /// input order.
            fn $batch(
                &self,
                template: $template,
            ) -> impl ::core::future::Future<Output = $crate::BatchResults<$item>> + Send;

            /// The same call with per-call headers and deadline.
            fn $batch_opts(
                &self,
                template: $template,
                headers: $crate::CallHeaders,
            ) -> impl ::core::future::Future<Output = $crate::BatchResults<$item>> + Send;
        }

        const _: () = {
            const _: fn(&mut $template, ::std::vec::Vec<::std::string::String>) =
                |template, ids| template.$ids = ids;

            fn shared(
                batched: &$crate::Batched<$client>,
            ) -> ::std::sync::Arc<$crate::Coalescer<$get_req, $template, $item>> {
                batched.__coalescer($single_method, |wire| {
                    let inner = batched.__inner();
                    let single_client = ::std::sync::Arc::clone(&inner);
                    let batch_client = inner;
                    let single_contract = batched.__validate_method($single_method, $batch_method);
                    let batch_contract = single_contract.clone();
                    $crate::Coalescer::new(
                        $crate::GetPlan {
                            single_method: $single_method,
                            batch_method: $batch_method,
                            single: ::std::sync::Arc::new(
                                move |request: $get_req,
                                      options: $crate::__private::CallOptions|
                                      -> $crate::Boxed<
                                    ::core::result::Result<
                                        $item,
                                        $crate::__private::ConnectError,
                                    >,
                                > {
                                    let client = ::std::sync::Arc::clone(&single_client);
                                    let contract = single_contract.clone();
                                    ::std::boxed::Box::pin(async move {
                                        contract?;
                                        client
                                            .$get_opts(request, options)
                                            .await
                                            .map(|response| response.into_owned())
                                    })
                                },
                            ),
                            batch: ::std::sync::Arc::new(
                                move |mut template: $template,
                                      ids: ::std::vec::Vec<::std::string::String>,
                                      options: $crate::__private::CallOptions|
                                      -> $crate::Boxed<
                                    ::core::result::Result<
                                        $crate::WireResults<$item>,
                                        $crate::__private::ConnectError,
                                    >,
                                > {
                                    let client = ::std::sync::Arc::clone(&batch_client);
                                    let contract = batch_contract.clone();
                                    ::std::boxed::Box::pin(async move {
                                        contract?;
                                        template.$ids = ids;
                                        let response = client.$batch_opts(template, options).await?.into_owned();
                                        let answers: $crate::WireResults<$item> = response
                                            .$results
                                            .into_iter()
                                            .map(|entry| {
                                                use $outcome as BatchOutcome;
                                                let outcome = match entry.outcome {
                                                    ::core::option::Option::Some(
                                                        BatchOutcome::Item(item),
                                                    ) => ::core::result::Result::Ok(
                                                        <$item as ::core::clone::Clone>::clone(
                                                            &item,
                                                        ),
                                                    ),
                                                    ::core::option::Option::Some(
                                                        BatchOutcome::Error(error),
                                                    ) => ::core::result::Result::Err(
                                                        $crate::item_error_to_connect(
                                                            &$crate::__batch_item_error!(error),
                                                        ),
                                                    ),
                                                    ::core::option::Option::None => {
                                                        ::core::result::Result::Err(
                                                            $crate::__private::ConnectError::internal(
                                                                "a batch result carries neither an item nor an error",
                                                            ),
                                                        )
                                                    }
                                                };
                                                (entry.id, outcome)
                                            })
                                            .collect();
                                        ::core::result::Result::Ok::<
                                            _,
                                            $crate::__private::ConnectError,
                                        >(answers)
                                    })
                                },
                            ),
                            id_of: |request: &$get_req| request.$id.clone(),
                            set_id: |request: &mut $get_req, id: &str| {
                                request.$id = id.to_owned();
                            },
                            encode_cleared: |request: &$get_req| {
                                let mut cleared = ::core::clone::Clone::clone(request);
                                cleared.$id = ::core::default::Default::default();
                                $crate::__private::buffa::Message::encode_to_vec(&cleared)
                            },
                            to_template: |request: &$get_req| {
                                <$template as ::core::convert::From<&$get_req>>::from(request)
                            },
                            template_bytes: |template: &$template| {
                                $crate::__private::template_bytes(
                                    $crate::__private::buffa::Message::encoded_len(template),
                                    $ids_field,
                                )
                            },
                        },
                        wire,
                    )
                })
            }

            impl $ext for $crate::Batched<$client> {
                fn $get(
                    &self,
                    request: $get_req,
                ) -> impl ::core::future::Future<Output = $crate::ItemResult<$item>> + Send {
                    self.$get_opts(request, $crate::CallHeaders::new())
                }

                fn $get_opts(
                    &self,
                    request: $get_req,
                    headers: $crate::CallHeaders,
                ) -> impl ::core::future::Future<Output = $crate::ItemResult<$item>> + Send {
                    let coalescer = shared(self);
                    async move { coalescer.get(request, headers).await }
                }

                fn $batch(
                    &self,
                    template: $template,
                ) -> impl ::core::future::Future<Output = $crate::BatchResults<$item>> + Send {
                    self.$batch_opts(template, $crate::CallHeaders::new())
                }

                fn $batch_opts(
                    &self,
                    mut template: $template,
                    headers: $crate::CallHeaders,
                ) -> impl ::core::future::Future<Output = $crate::BatchResults<$item>> + Send {
                    let coalescer = shared(self);
                    async move {
                        let ids = ::core::mem::take(&mut template.$ids);
                        let bytes = $crate::__private::template_bytes(
                            $crate::__private::buffa::Message::encoded_len(&template),
                            $ids_field,
                        );
                        coalescer.explicit(template, &ids, &headers, bytes).await
                    }
                }
            }
        };
    };
    (
        ext: $ext:ident,
        client: $client:ty,
        list: {
            method: $single_method:literal,
            with_options: $list_opts:ident,
            request: $list_req:ty,
            response: $list_resp:ty $(,)?
        },
        batch: {
            method: $batch_method:literal,
            name: $batch:ident,
            with_options: $batch_opts:ident,
            template: $template:ty,
            parents: $parents:ident,
            parents_field: $parents_field:literal,
            groups: $groups:ident,
            group_parent: $group_parent:ident $(,)?
        } $(,)?
    ) => {
        /// Grouped batch lists of one method pair.
        pub trait $ext {
            /// A grouped batch list: one group per parent in input order,
            /// each continuing through the page iterator.
            fn $batch(
                &self,
                template: $template,
            ) -> impl ::core::future::Future<
                Output = $crate::BatchGroups<$list_req, $list_resp>,
            > + Send;

            /// The same call with per-call headers and deadline.
            fn $batch_opts(
                &self,
                template: $template,
                headers: $crate::CallHeaders,
            ) -> impl ::core::future::Future<
                Output = $crate::BatchGroups<$list_req, $list_resp>,
            > + Send;
        }

        const _: () = {
            const _: fn(&mut $template, ::std::vec::Vec<::std::string::String>) =
                |template, parents| template.$parents = parents;

            impl $ext for $crate::Batched<$client> {
                fn $batch(
                    &self,
                    template: $template,
                ) -> impl ::core::future::Future<
                    Output = $crate::BatchGroups<$list_req, $list_resp>,
                > + Send {
                    self.$batch_opts(template, $crate::CallHeaders::new())
                }

                fn $batch_opts(
                    &self,
                    mut template: $template,
                    headers: $crate::CallHeaders,
                ) -> impl ::core::future::Future<
                    Output = $crate::BatchGroups<$list_req, $list_resp>,
                > + Send {
                    let wire = self.__wire();
                    let single_client = self.__inner();
                    let batch_client = self.__inner();
                    let batch_contract = self.__validate_method($single_method, $batch_method);
                    async move {
                        let parents = ::core::mem::take(&mut template.$parents);
                        let spec = $crate::GroupedSpec {
                            rpc: $batch_method,
                            single: $single_method,
                            idempotency: $crate::__private::Idempotency::NoSideEffects,
                            batch: ::std::sync::Arc::new(
                                move |mut template: $template,
                                      parents: ::std::vec::Vec<::std::string::String>,
                                      options: $crate::__private::CallOptions|
                                      -> $crate::Boxed<
                                    ::core::result::Result<
                                        $crate::WireResults<
                                            $crate::FirstPage<
                                                <$list_resp as $crate::__private::PagedResponse>::Item,
                                            >,
                                        >,
                                        $crate::__private::ConnectError,
                                    >,
                                > {
                                    let client = ::std::sync::Arc::clone(&batch_client);
                                    let contract = batch_contract.clone();
                                    ::std::boxed::Box::pin(async move {
                                        contract?;
                                        template.$parents = parents;
                                        let response = client.$batch_opts(template, options).await?.into_owned();
                                        let answers: $crate::WireResults<
                                            $crate::FirstPage<
                                                <$list_resp as $crate::__private::PagedResponse>::Item,
                                            >,
                                        > = response
                                            .$groups
                                            .into_iter()
                                            .map(|group| {
                                                let outcome = match group.error.into_option() {
                                                    ::core::option::Option::Some(error) => {
                                                        ::core::result::Result::Err(
                                                            $crate::item_error_to_connect(
                                                                &$crate::__batch_item_error!(error),
                                                            ),
                                                        )
                                                    }
                                                    ::core::option::Option::None => {
                                                        ::core::result::Result::Ok(
                                                            $crate::FirstPage {
                                                                items: group.items,
                                                                next_cursor: group
                                                                    .page
                                                                    .into_option()
                                                                    .map(|page| page.next_cursor)
                                                                    .unwrap_or_default(),
                                                            },
                                                        )
                                                    }
                                                };
                                                (group.$group_parent, outcome)
                                            })
                                            .collect();
                                        ::core::result::Result::Ok::<
                                            _,
                                            $crate::__private::ConnectError,
                                        >(answers)
                                    })
                                },
                            ),
                            single_call: ::std::sync::Arc::new(
                                move |request: $list_req,
                                      options: $crate::__private::CallOptions|
                                      -> $crate::Boxed<
                                    ::core::result::Result<
                                        $list_resp,
                                        $crate::__private::ConnectError,
                                    >,
                                > {
                                    let client = ::std::sync::Arc::clone(&single_client);
                                    ::std::boxed::Box::pin(async move {
                                        client
                                            .$list_opts(request, options)
                                            .await
                                            .map(|response| response.into_owned())
                                    })
                                },
                            ),
                            template_bytes: $crate::__private::template_bytes(
                                $crate::__private::buffa::Message::encoded_len(&template),
                                $parents_field,
                            ),
                        };
                        $crate::run_grouped(&wire, &spec, template, &parents, &headers).await
                    }
                }
            }
        };
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::element_bytes;

    #[test]
    fn fields_up_to_fifteen_take_a_one_byte_tag() {
        assert_eq!(tag_len(1), 1);
        assert_eq!(tag_len(15), 1);
    }

    #[test]
    fn fields_from_sixteen_take_a_two_byte_tag() {
        assert_eq!(tag_len(16), 2);
        assert_eq!(tag_len(2047), 2);
        assert_eq!(tag_len(2048), 3);
    }

    #[test]
    fn field_fifteen_reserves_nothing() {
        assert_eq!(template_bytes(40, 15), 40);
    }

    #[test]
    fn field_sixteen_reserves_one_extra_byte_per_element_of_a_full_chunk() {
        assert_eq!(template_bytes(40, 16), 40 + MAX_ITEMS);
    }

    #[test]
    fn the_reserve_covers_the_real_size_of_a_full_chunk() {
        let id = "0123456789abcdef";
        for field in [15, 16, 2048] {
            let planned = template_bytes(0, field) + MAX_ITEMS * element_bytes(id);
            let real = MAX_ITEMS * (tag_len(field) + element_bytes(id) - 1);
            assert!(
                planned >= real,
                "field {field}: planned {planned} < real {real}"
            );
        }
    }
}
