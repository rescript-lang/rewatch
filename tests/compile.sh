#!/bin/bash

cd $(dirname $0)
source "./utils.sh"
cd "$1" || exit

bold "Test: It should compile"

if rewatch clean
then
  success "Repo Cleaned"
else 
  error "Error Cleaning Repo"
  exit 1
fi

if rewatch build &> /dev/null
then
  success "Repo Built"
else 
  error "Error Building Repo"
  exit 1
fi


if git diff --exit-code ./; 
then
  success "testrepo has no changes"
else 
  error "Build has changed"
  exit 1
fi

node ./packages/main/src/Main.mjs > ./packages/main/src/output.txt

mv ./packages/main/src/Main.res ./packages/main/src/Main2.res
rewatch build --no-timing=true &> ../snapshots/rename-file-"$1".txt
mv ./packages/main/src/Main2.res ./packages/main/src/Main.res
rewatch build &>  /dev/null
mv ./packages/main/src/ModuleWithInterface.resi ./packages/main/src/ModuleWithInterface2.resi
rewatch build --no-timing=true &> ../snapshots/rename-interface-file-"$1".txt
mv ./packages/main/src/ModuleWithInterface2.resi ./packages/main/src/ModuleWithInterface.resi
rewatch build &> /dev/null
mv ./packages/main/src/ModuleWithInterface.res ./packages/main/src/ModuleWithInterface2.res
rewatch build --no-timing=true &> ../snapshots/rename-file-with-interface-"$1".txt
mv ./packages/main/src/ModuleWithInterface2.res ./packages/main/src/ModuleWithInterface.res
rewatch build &> /dev/null

# when deleting a file that other files depend on, the compile should fail
rm packages/dep02/src/Dep02.res
rewatch build --no-timing=true &> ../snapshots/remove-file-"$1".txt
# replace the absolute path so the snapshot is the same on all machines
replace "s/$(pwd | sed "s/\//\\\\\//g")//g" ../snapshots/remove-file-"$1".txt
git checkout -- packages/dep02/src/Dep02.res
rewatch build &> /dev/null

# it should show an error when we have a dependency cycle
echo 'Dep01.log()' >> packages/new-namespace/src/NS_alias.res
rewatch build --no-timing=true &> ../snapshots/dependency-cycle-"$1".txt
git checkout -- packages/new-namespace/src/NS_alias.res
rewatch build &> /dev/null

# it should not loop (we had an infinite loop when clean building with a cycle)
rewatch clean &> /dev/null
echo 'Dep01.log()' >> packages/new-namespace/src/NS_alias.res
git checkout -- packages/new-namespace/src/NS_alias.res
rewatch build &> /dev/null

# make sure we don't have changes in the test repo
if git diff --exit-code ./; 
then
  success "Output is correct"
else 
  error "Output is incorrect"
  exit 1
fi

# make sure there are no new files created by the build
# this could happen because of not cleaning up .mjs files
# after we rename files
new_files=$(git ls-files --others --exclude-standard ./)
if [[ $new_files = "" ]];
then
  success "No new files created"
else 
  error "❌ - New files created"
  printf "${new_files}\n"
  exit 1
fi

# see if the snapshots have changed
changed_snapshots=$(git ls-files  --modified ../snapshots)
if git diff --exit-code ../snapshots &> /dev/null; 
then
  success "Snapshots are correct"
else 
  error "Snapshots are incorrect:"
  # print filenames in the snapshot dir call bold with the filename
  # and then cat their contents
  printf "\n\n"
  for file in $changed_snapshots; do
    bold "$file"
    # show diff of file vs contents in git
    git diff "$file" "$file"
    printf "\n\n"
  done

  exit 1
fi
