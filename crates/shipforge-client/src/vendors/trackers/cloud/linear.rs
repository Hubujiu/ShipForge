use super::*;

const ISSUE_FIELDS: &str = "id identifier title description url updatedAt priority state { id name type } assignee { id } creator { id } project { id }";
fn graphql(api: &Cloud, operation: &str, query: &str, vars: Value) -> Result<Value> {
    api.request(
        "POST",
        "/graphql",
        &[],
        &json!({"operationName":operation,"query":query,"variables":vars}),
    )
}
pub(super) fn accounts(api: &Cloud) -> Result<Vec<Value>> {
    let org = graphql(
        api,
        "CurrentOrganization",
        "query CurrentOrganization { organization { id name } }",
        json!({}),
    )?;
    Ok(vec![
        json!({"id":id(&org["organization"]["id"])?,"name":org["organization"]["name"]}),
    ])
}
fn pages(
    api: &Cloud,
    operation: &str,
    query: &str,
    field: &str,
    mut vars: Value,
) -> Result<Vec<Value>> {
    let mut seen = HashSet::new();
    let mut records = Records::new();
    loop {
        let response = graphql(api, operation, query, vars.clone())?;
        let block = &response[field];
        for row in array(block, "nodes")? {
            records.add(row.clone())?;
        }
        match next_page(&block["pageInfo"], &mut seen)? {
            Some(cursor) => vars["after"] = json!(cursor),
            None => break,
        }
    }
    Ok(records.finish())
}
pub(super) fn projects(api: &Cloud, size: usize) -> Result<Vec<Value>> {
    pages(
        api,
        "DiscoverProjects",
        "query DiscoverProjects($first:Int!,$after:String){ projects(first:$first,after:$after,includeArchived:true){nodes{id name url} pageInfo{hasNextPage endCursor}}}",
        "projects",
        json!({"first":size,"after":null}),
    )
}
pub(super) fn issues(
    api: &Cloud,
    scope: &str,
    size: usize,
    since: Option<i64>,
) -> Result<Vec<Value>> {
    let mut filter = json!({"project":{"id":{"eq":scope}}});
    if let Some(since) = since {
        let date = chrono::DateTime::from_timestamp_millis(since)
            .ok_or("invalid: invalid import checkpoint")?;
        filter["updatedAt"] = json!({"gte":date.to_rfc3339()});
    }
    let query = format!(
        "query ReadIssues($first:Int!,$after:String,$filter:IssueFilter!){{ issues(first:$first,after:$after,orderBy:updatedAt,filter:$filter,includeArchived:true){{ nodes{{ {ISSUE_FIELDS} parent{{id}} relations(first:50){{nodes{{id type relatedIssue{{id}}}} pageInfo{{hasNextPage endCursor}}}} }} pageInfo{{ hasNextPage endCursor }} }} }}"
    );
    let mut records = pages(
        api,
        "ReadIssues",
        &query,
        "issues",
        json!({"first":size,"after":null,"filter":filter}),
    )?;
    for row in &mut records {
        if id(&row["project"]["id"])? != scope {
            return Err("forbidden: Linear returned an issue from another project".into());
        }
        let key = id(&row["id"])?;
        let relation = &row["relations"];
        let mut all = Records::new();
        for edge in array(relation, "nodes")? {
            all.add(edge.clone())?;
        }
        let mut seen = HashSet::new();
        let mut next = next_page(&relation["pageInfo"], &mut seen)?;
        while let Some(cursor) = next {
            let data = graphql(
                api,
                "ReadRelations",
                "query ReadRelations($id:String!,$after:String!){issue(id:$id){id project{id} relations(first:50,after:$after){nodes{id type relatedIssue{id}} pageInfo{hasNextPage endCursor}}}}",
                json!({"id":key,"after":cursor}),
            )?;
            if data["issue"]["id"] != key || data["issue"]["project"]["id"] != scope {
                return Err(
                    "forbidden: Linear issue changed project during relation pagination".into(),
                );
            }
            let rel = &data["issue"]["relations"];
            for edge in array(rel, "nodes")? {
                all.add(edge.clone())?;
            }
            next = next_page(&rel["pageInfo"], &mut seen)?;
        }
        row["relations"] = json!(all.finish());
    }
    Ok(records)
}
pub(super) fn detail(api: &Cloud, key: &str) -> Result<Value> {
    let q = format!("query IssueDetails($id:String!){{issue(id:$id){{ {ISSUE_FIELDS} }} }}");
    Ok(graphql(api, "IssueDetails", &q, json!({"id":key}))?["issue"].take())
}
pub(super) fn status(api: &Cloud, key: &str, target: &str) -> Result<()> {
    let v = graphql(
        api,
        "SetIssueState",
        "mutation SetIssueState($id:String!,$state:String!){ issueUpdate(id:$id,input:{stateId:$state}) { success issue{id} } }",
        json!({"id":key,"state":target}),
    )?;
    if v["issueUpdate"]["success"] != true || v["issueUpdate"]["issue"]["id"] != key {
        return Err("vendor: Linear did not confirm the workflow change".into());
    }
    Ok(())
}
pub(super) fn comment(api: &Cloud, key: &str, body: &str, confirm: Option<&str>) -> Result<String> {
    if let Some(known) = confirm {
        let comment = graphql(
            api,
            "CheckComment",
            "query CheckComment($id:String!){ comment(id:$id){id body issue{id}} }",
            json!({"id":known}),
        )?;
        let r = &comment["comment"];
        if r["id"] != known || r["body"] != body || r["issue"]["id"] != key {
            return Err("conflict: upstream comment differs from this publication".into());
        }
        return Ok(known.into());
    }
    let v = graphql(
        api,
        "PublishComment",
        "mutation PublishComment($id:String!,$body:String!){ commentCreate(input:{issueId:$id,body:$body}){success comment{id}} }",
        json!({"id":key,"body":body}),
    )?;
    if v["commentCreate"]["success"] != true {
        return Err("vendor: Linear did not confirm the comment".into());
    }
    id(&v["commentCreate"]["comment"]["id"])
}
