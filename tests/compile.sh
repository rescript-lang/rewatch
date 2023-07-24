source "./utils.sh"
cd ../testrepo

bold "Test: It should compile"

if rewatch clean &> /dev/null;
then
  success "Repo Cleaned"
else 
  error "Error Cleaning Repo"
  exit 1
fi

if rewatch &> /dev/null; 
then
  success "Repo Built"
else 
  error "Error Building Repo"
  exit 1
fi


if git diff --exit-code ./; 
then
  success "Testrepo has no changes"
else 
  error "Build has changed"
  exit 1
fi

node ./packages/main/src/Main.mjs > ./packages/main/src/output.txt

mv ./packages/main/src/Main.res ./packages/main/src/Main2.res
rewatch build --no-timing=true &> ../tests/snapshots/rename-file.txt
mv ./packages/main/src/Main2.res ./packages/main/src/Main.res
rewatch build &>  /dev/null
mv ./packages/main/src/ModuleWithInterface.resi ./packages/main/src/ModuleWithInterface2.resi
rewatch build --no-timing=true &> ../tests/snapshots/rename-interface-file.txt
mv ./packages/main/src/ModuleWithInterface2.resi ./packages/main/src/ModuleWithInterface.resi
rewatch build &> /dev/null
mv ./packages/main/src/ModuleWithInterface.res ./packages/main/src/ModuleWithInterface2.res
rewatch build --no-timing=true &> ../tests/snapshots/rename-file-with-interface.txt
mv ./packages/main/src/ModuleWithInterface2.res ./packages/main/src/ModuleWithInterface.res
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
changed_snapshots=$(git ls-files  --modified ../tests/snapshots)
if git diff --exit-code ../tests/snapshots &> /dev/null; 
then
  success "Snapshots are correct"
else 
  error "Snapshots are incorrect:"
  printf "${changed_snapshots}\n"
  exit 1
fi